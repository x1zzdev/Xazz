//! xazz-exec/src/dl.rs — Burn 딥러닝 실행 엔진 (v0.4)
//!
//! .xzz 스크립트의 `model {}` 선언과 `run v |> train(...)` 구문을 실제 Burn
//! 신경망 학습으로 변환하여 실행한다.
//!
//! 백엔드: burn-ndarray (순수 Rust CPU). `Backend` 제네릭으로 구조화되어 있어
//! torch / wgpu 로 전환 시 이 모듈의 `AD`/`Plain` 타입 별칭만 교체하면 된다.
//!
//!   - AD    = NdArrayAutodiff<f32>  (학습용: autodiff 그래프 활성)
//!   - Plain = NdArray<f32>          (추론용)

use std::collections::HashMap;

use burn::{
    backend::Autodiff,
    module::{AutodiffModule, Module},
    nn::{DropoutConfig, Linear, LinearConfig},
    optim::{AdamConfig, GradientsParams, Optimizer},
    record::{FullPrecisionSettings, PrettyJsonFileRecorder},
    tensor::{
        Device, Tensor, TensorData,
        activation::{relu, sigmoid, softmax, tanh},
        backend::Backend,
    },
};
use burn_ndarray::NdArray;
use polars::prelude::{Column, DataFrame};
use xazz_compiler::ast::{LayerKind, TrainConfig};

use crate::tensor_bridge::{extract_data, series_to_f32};

/// 학습용 autodiff 백엔드 (CPU): NdArray + Autodiff 래퍼.
pub type AD = Autodiff<NdArray<f32>>;
/// 추론용 순수 백엔드 (CPU).
pub type Plain = NdArray<f32>;

/// dsL 모델 블록의 활성화/정규화 레이어 종류.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Activation {
    None,
    ReLU,
    Sigmoid,
    Tanh,
    Softmax,
    Dropout(f64),
}

/// 레이어별 활성화 함수 적용.
/// Dropout 은 학습(training=true)에서만 적용하고, 추론에서는 항등 함수로 통과시킨다.
fn apply_activation<B: Backend, const D: usize>(
    act: &Activation,
    x: Tensor<B, D>,
    training: bool,
) -> Tensor<B, D> {
    match act {
        Activation::None => x,
        Activation::ReLU => relu(x),
        Activation::Sigmoid => sigmoid(x),
        Activation::Tanh => tanh(x),
        Activation::Softmax => softmax(x, 1),
        Activation::Dropout(prob) => {
            if training {
                DropoutConfig::new(*prob).init().forward(x)
            } else {
                x
            }
        }
    }
}

/// DSL `model { Dense -> ReLU -> ... }` 를 동적 다층 퍼셉트론(MLP)으로 표현한 Burn 모듈.
// (Burn Module derive 가 Clone 을 제공한다)
#[derive(Module, Debug)]
pub struct Mlp<B: Backend> {
    /// Dense(units) 레이어 시퀀스.
    layers: Vec<Linear<B>>,
    /// 각 Dense 뒤에 적용할 활성화 함수 (마지막 레이어 포함).
    #[module(skip)]
    activations: Vec<Activation>,
    /// 학습 모드 여부 — Dropout 적용을 결정한다 (추론 시 false).
    #[module(skip)]
    training: bool,
}

impl<B: Backend> Mlp<B> {
    /// 순전파: [batch, input_dim] → [batch, output_dim].
    fn forward<const D: usize>(&self, input: Tensor<B, D>) -> Tensor<B, D> {
        let mut x = input;
        for i in 0..self.layers.len() {
            x = apply_activation(&self.activations[i], self.layers[i].forward(x), self.training);
        }
        x
    }
}

/// DSM 레이어 목록과 입력 차원으로 MLP 를 구성한다.
fn build_mlp<B: Backend>(
    layers: &[LayerKind],
    input_dim: usize,
    device: &Device<B>,
) -> Result<Mlp<B>, String> {
    let mut linears: Vec<Linear<B>> = Vec::new();
    let mut activations: Vec<Activation> = Vec::new();
    let mut cur = input_dim;

    for layer in layers {
        match layer {
            LayerKind::Dense(n) if *n > 0 => {
                linears.push(LinearConfig::new(cur, *n).init(device));
                activations.push(Activation::None); // DSL 활성화가 있으면 덮어씀
                cur = *n;
            }
            LayerKind::Dense(_) => {
                return Err("Dense 레이어의 유닛 수는 1 이상이어야 합니다.".into());
            }
            LayerKind::ReLU => set_activation(&mut activations, Activation::ReLU),
            LayerKind::Sigmoid => set_activation(&mut activations, Activation::Sigmoid),
            LayerKind::Tanh => set_activation(&mut activations, Activation::Tanh),
            LayerKind::Softmax => set_activation(&mut activations, Activation::Softmax),
            LayerKind::Dropout(r) => set_activation(&mut activations, Activation::Dropout(*r)),
            // BatchNorm(1D MLP)은 Burn v2D BatchNorm 과 레이아웃이 달라 생략한다.
            LayerKind::BatchNorm => { /* pass-through */ }
        }
    }

    if linears.is_empty() {
        return Err("모델에 Dense 레이어가 하나도 없습니다.".into());
    }
    Ok(Mlp {
        layers: linears,
        activations,
        training: true,
    })
}

/// 마지막 Dense 뒤에 활성화를 기록한다 (연속 활성화는 마지막 것만 적용).
fn set_activation(acts: &mut Vec<Activation>, act: Activation) {
    if let Some(last) = acts.last_mut() {
        *last = act;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 학습 결과 리포트
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct TrainReport {
    pub model_name: String,
    pub target: String,
    pub feature_names: Vec<String>,
    pub input_dim: usize,
    pub output_dim: usize,
    pub num_params: usize,
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f32,
    pub final_train_loss: f64,
    pub final_val_loss: Option<f64>,
    pub predictions: Vec<f64>,
    pub targets: Vec<f64>,
    pub checkpoint_path: String,
}

/// 학습된 모델 — 예측(predict)에 필요한 표준화 통계까지 보유한다.
#[derive(Debug, Clone)]
pub struct TrainedModel {
    /// 추론용 순수 모델 (autodiff 그래프 없음).
    pub model: Mlp<Plain>,
    /// 학습 결과 리포트 (마커/로그용).
    pub report: TrainReport,
    /// 특성 컬럼 순서 (표준화 통계와 1:1 대응).
    pub feature_names: Vec<String>,
    /// 특성별 평균 (z-score).
    pub fmean: Vec<f64>,
    /// 특성별 표준편차 (z-score).
    pub fstd: Vec<f64>,
    /// 학습 대상 컬럼.
    pub target: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// 공개 API
// ─────────────────────────────────────────────────────────────────────────────

/// `dataset |> train(<model>, target: "...", ...)` 를 실행하고 학습된 모델을 반환한다.
pub fn train(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
) -> Result<TrainedModel, String> {
    let (feature_names, features, targets) = extract_data(df, &config.target)?;
    let n = features.len();
    let input_dim = feature_names.len();
    if input_dim == 0 {
        return Err("학습 가능한 숫자형 특성(컬럼)이 없습니다.".into());
    }
    if n == 0 {
        return Err("학습 데이터가 비어 있습니다.".into());
    }

    // ── 특성 표준화 통계 (NaN → 평균 대체) ─────────────────────────────────
    let mut fmean = vec![0f64; input_dim];
    let mut fstd = vec![1f64; input_dim];
    for j in 0..input_dim {
        let (mut s, mut c) = (0f64, 0usize);
        for i in 0..n {
            if let Some(v) = features.get(i).and_then(|row| row.get(j)) {
                let v = *v as f64;
                if v.is_finite() {
                    s += v;
                    c += 1;
                }
            }
        }
        fmean[j] = if c > 0 { s / c as f64 } else { 0.0 };
    }
    for j in 0..input_dim {
        let (mut s, mut c) = (0f64, 0usize);
        for i in 0..n {
            if let Some(v) = features.get(i).and_then(|row| row.get(j)) {
                let d = *v as f64 - fmean[j];
                if d.is_finite() {
                    s += d * d;
                    c += 1;
                }
            }
        }
        fstd[j] = if c > 1 {
            (s / (c - 1) as f64).max(1e-8).sqrt()
        } else {
            1.0
        };
    }

    let mut xs = Vec::with_capacity(n * input_dim);
    for i in 0..n {
        for j in 0..input_dim {
            let mut v = features[i][j] as f64;
            if !v.is_finite() {
                v = fmean[j];
            }
            xs.push(((v - fmean[j]) / fstd[j]) as f32);
        }
    }

    let tmean: f64 = {
        let (mut s, mut c) = (0f64, 0usize);
        for &t in &targets {
            if t.is_finite() {
                s += t as f64;
                c += 1;
            }
        }
        if c > 0 { s / c as f64 } else { 0.0 }
    };
    let ys: Vec<f32> = targets
        .iter()
        .map(|&t| {
            if t.is_finite() {
                t as f32
            } else {
                tmean as f32
            }
        })
        .collect();

    // ── train / validation 분할 ────────────────────────────────────────────
    let val_split = config.validation_split.unwrap_or(0.0).clamp(0.0, 0.9);
    let val_n = (n as f64 * val_split) as usize;
    let train_n = n - val_n;
    let val_idx: Vec<usize> = (train_n..n).collect();

    let device: Device<AD> = Default::default();
    let mut model = build_mlp::<AD>(layers, input_dim, &device)?;

    let batch_size = config.batch_size.unwrap_or(train_n.max(1));
    let lr = config.learning_rate as f64;
    let mut optim = AdamConfig::new().init::<AD, _>();

    let make_batch = |idx: &[usize]| -> Option<(Tensor<AD, 2>, Tensor<AD, 2>)> {
        if idx.is_empty() {
            return None;
        }
        let b = idx.len();
        let mut xv = Vec::with_capacity(b * input_dim);
        let mut yv = Vec::with_capacity(b);
        for &i in idx {
            for j in 0..input_dim {
                xv.push(xs[i * input_dim + j]);
            }
            yv.push(ys[i]);
        }
        let x = Tensor::<AD, 2>::from_data(TensorData::new(xv, [b, input_dim]), &device);
        let y = Tensor::<AD, 2>::from_data(TensorData::new(yv, [b, 1]), &device);
        Some((x, y))
    };

    let mut final_train_loss = f64::NAN;
    let mut final_val_loss: Option<f64> = None;

    for epoch in 0..config.epochs {
        // 결정론적 셔플 (epoch 시드)
        let mut order: Vec<usize> = (0..train_n).collect();
        let mut seed = epoch as u64 + 1;
        for i in (1..order.len()).rev() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (seed >> 33) as usize % (i + 1);
            order.swap(i, j);
        }

        let mut epoch_loss = 0f64;
        let mut steps = 0usize;
        for chunk in order.chunks(batch_size) {
            let (x, y) = match make_batch(chunk) {
                Some(v) => v,
                None => continue,
            };
            let out = model.forward(x);
            let loss = ((out - y).powf_scalar(2.0)).mean();
            let loss_val = loss
                .clone()
                .into_data()
                .to_vec::<f32>()
                .map(|v| v[0] as f64)
                .unwrap_or(f64::NAN);

            let grads = loss.backward();
            let grads = GradientsParams::from_grads(grads, &model);
            model = optim.step(lr, model, grads);

            epoch_loss += loss_val;
            steps += 1;
        }
        final_train_loss = if steps > 0 {
            epoch_loss / steps as f64
        } else {
            f64::NAN
        };

        if !val_idx.is_empty() {
            if let Some((xv, yv)) = make_batch(&val_idx) {
                let vout = model.forward(xv);
                let vloss = ((vout - yv).powf_scalar(2.0)).mean();
                final_val_loss = vloss.into_data().to_vec::<f32>().map(|v| v[0] as f64).ok();
            }
        }

        let val_line = final_val_loss
            .map(|v| format!("  val_loss = {v:.6}"))
            .unwrap_or_default();
        println!(
            "  [Epoch {:>3}/{}]  train_loss = {final_train_loss:.6}{val_line}",
            epoch + 1,
            config.epochs
        );
    }

    // ── 샘플 예측 (in-sample) ──────────────────────────────────────────────
    let n_pred = n.min(10);
    let device_plain: Device<Plain> = Default::default();
    let mut xv = Vec::with_capacity(n_pred * input_dim);
    for i in 0..n_pred {
        for j in 0..input_dim {
            xv.push(xs[i * input_dim + j]);
        }
    }
    let xp = Tensor::<Plain, 2>::from_data(TensorData::new(xv, [n_pred, input_dim]), &device_plain);
    let mut valid_model = model.valid();
    valid_model.training = false;
    let pred_t = valid_model.forward(xp);
    let preds = pred_t.into_data().to_vec::<f32>().unwrap_or_default();
    let predictions: Vec<f64> = preds.iter().map(|&v| v as f64).collect();
    let targets_out: Vec<f64> = (0..n_pred).map(|i| ys[i] as f64).collect();

    let num_params = model.num_params();
    let output_dim = model
        .layers
        .last()
        .map(|l| l.weight.shape().dims::<2>()[1])
        .unwrap_or(1);

    // ── 체크포인트 저장 ────────────────────────────────────────────────────
    let ckpt_dir = "checkpoints";
    std::fs::create_dir_all(ckpt_dir)
        .map_err(|e| format!("checkpoints/ 디렉토리 생성 실패: {e}"))?;
    let ckpt = format!("{ckpt_dir}/{model_name}");
    let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
    valid_model
        .clone()
        .save_file(&ckpt, &recorder)
        .map_err(|e| format!("체크포인트 저장 실패: {e}"))?;

    let report = TrainReport {
        model_name: model_name.to_string(),
        target: config.target.clone(),
        feature_names: feature_names.clone(),
        input_dim,
        output_dim,
        num_params,
        epochs: config.epochs,
        batch_size,
        learning_rate: lr as f32,
        final_train_loss,
        final_val_loss,
        predictions,
        targets: targets_out,
        checkpoint_path: format!("{ckpt}.json"),
    };

    Ok(TrainedModel {
        model: valid_model,
        report,
        feature_names,
        fmean,
        fstd,
        target: config.target.clone(),
    })
}

/// `dataset |> predict(model_var, as: "col")` — 학습된 모델로 예측 컬럼을 추가한다.
///
/// 예측 컬럼명 기본값: `<target>_pred`.
pub fn predict(
    trained: &TrainedModel,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<DataFrame, String> {
    let feature_count = trained.feature_names.len();
    let n = df.height();
    if feature_count == 0 {
        return Err("모델에 특성 정보가 없습니다. 먼저 train()으로 학습하세요.".into());
    }
    if n == 0 {
        return Err("예측할 데이터가 비어 있습니다.".into());
    }

    let mut col_vecs: Vec<Vec<f32>> = Vec::with_capacity(feature_count);
    for j in 0..feature_count {
        let name = &trained.feature_names[j];
        let col = df
            .column(name.as_str())
            .map_err(|e| format!("예측 특성 컬럼 '{name}' 접근 실패: {e}"))?;
        col_vecs.push(series_to_f32(col));
    }

    let mut xs = Vec::with_capacity(n * feature_count);
    for i in 0..n {
        for j in 0..feature_count {
            let mut v = col_vecs[j][i] as f64;
            if !v.is_finite() {
                v = trained.fmean[j];
            }
            xs.push(((v - trained.fmean[j]) / trained.fstd[j]) as f32);
        }
    }

    let device: Device<Plain> = Default::default();
    let x = Tensor::<Plain, 2>::from_data(TensorData::new(xs, [n, feature_count]), &device);
    let mut infer_model = trained.model.clone();
    infer_model.training = false;
    let pred_t = infer_model.forward(x);
    let preds = pred_t.into_data().to_vec::<f32>().unwrap_or_default();

    let out_col = match as_col {
        Some(c) => c.to_string(),
        None => format!("{}_pred", trained.target),
    };
    let pred_f64: Vec<f64> = preds.iter().map(|&v| v as f64).collect();

    let mut out = df.clone();
    out.with_column(Column::new(out_col.into(), pred_f64))
        .map_err(|e| format!("예측 컬럼 추가 실패: {e}"))?;
    Ok(out)
}

/// Layered model registry helper: <model 이름, LayerKind 목록>.
pub type ModelRegistry = HashMap<String, Vec<LayerKind>>;
