//! xazz-exec/src/tensor_bridge.rs — Polars ↔ Burn 데이터 변환 인터페이스 (v0.6)
//!
//! 데이터 가속 계층(Polars DataFrame)과 딥러닝 컴파일 계층(Burn Tensor) 사이의
//! 공식 경계. 학습/추론 경로의 모든 DataFrame → Tensor 변환은 이 모듈을 거친다.
//!
//! ## dtype 표준화 규약 (Polars → 텐서)
//!
//! | Polars DataType                  | 텐서 원소 | 결측(null) 처리 |
//! |----------------------------------|-----------|------------------|
//! | Float64 / Float32                | f32       | NaN 으로 전달    |
//! | Int8~64 / UInt8~64               | f32       | NaN 으로 전달    |
//! | String / Bool / Date 등 비숫자   | 변환 제외 | —                |
//!
//! 결측을 NaN 으로 흘려보내는 이유: 대체 전략(평균 등)은 호출 측(dl::train 표준화
//! 단계)의 책임이며, 브리지는 값을 왜곡하지 않고 그대로 전달한다.

use burn::tensor::TensorData;
use polars::prelude::{Column, DataFrame, DataType};

// ─────────────────────────────────────────────────────────────────────────────
// dtype 표준화
// ─────────────────────────────────────────────────────────────────────────────

/// 텐서 변환 대상이 되는 숫자형 dtype 인지 판정한다.
pub fn is_numeric_dtype(dt: &DataType) -> bool {
    matches!(
        dt,
        DataType::Float64
            | DataType::Float32
            | DataType::Int64
            | DataType::Int32
            | DataType::Int16
            | DataType::Int8
            | DataType::UInt64
            | DataType::UInt32
            | DataType::UInt16
            | DataType::UInt8
    )
}

/// 컬럼 값을 f32 벡터로 변환한다. null → NaN. (dtype 표준화 규약 참조)
pub fn series_to_f32(col: &Column) -> Vec<f32> {
    // 개별 dtype 분기 대신 Float64 캐스팅 경로로 일원화 — 규약 표의 단일 구현점.
    match col.cast(&DataType::Float64) {
        Ok(casted) => match casted.f64() {
            Ok(ca) => ca
                .iter()
                .map(|o| o.map(|v| v as f32).unwrap_or(f32::NAN))
                .collect(),
            Err(_) => vec![f32::NAN; col.len()],
        },
        Err(_) => vec![f32::NAN; col.len()],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 특성/타겟 추출 (학습 경로)
// ─────────────────────────────────────────────────────────────────────────────

/// 타겟 컬럼을 제외한 숫자형 컬럼을 특성으로, 타겟 컬럼을 레이블로 추출한다.
///
/// 반환: (특성 컬럼명 순서, 행 단위 특성값, 타겟값)
/// 컬럼명 순서는 이후 표준화 통계·예측 입력과 1:1 로 대응하므로 보존이 계약이다.
pub fn extract_data(
    df: &DataFrame,
    target: &str,
) -> Result<(Vec<String>, Vec<Vec<f32>>, Vec<f32>), String> {
    let names = df.get_column_names();
    let mut feature_names: Vec<String> = Vec::new();
    let mut target_col: Option<Column> = None;

    for name in &names {
        let col = df
            .column(name.as_str())
            .map_err(|e| format!("컬럼 접근 실패: {e}"))?;
        if !is_numeric_dtype(col.dtype()) {
            continue;
        }
        if name.as_str() == target {
            target_col = Some(col.clone());
        } else {
            feature_names.push(name.to_string());
        }
    }

    let target_series = target_col.ok_or_else(|| {
        format!(
            "타겟 컬럼 '{target}' 이 존재하지 않거나 숫자형이 아닙니다. 사용 가능한 숫자형 컬럼: {}",
            names
                .iter()
                .filter(|n| {
                    df.column(n.as_str())
                        .map(|c| is_numeric_dtype(c.dtype()))
                        .unwrap_or(false)
                })
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let n = df.height();
    let feature_count = feature_names.len();
    let target_vec = series_to_f32(&target_series);

    let mut features: Vec<Vec<f32>> = vec![Vec::with_capacity(feature_count); n];
    for name in &names {
        if name.as_str() == target {
            continue;
        }
        let col = df
            .column(name.as_str())
            .map_err(|e| format!("컬럼 접근 실패: {e}"))?;
        if !is_numeric_dtype(col.dtype()) {
            continue;
        }
        let vals = series_to_f32(col);
        for i in 0..n {
            features[i].push(vals[i]);
        }
    }

    Ok((feature_names, features, target_vec))
}

// ─────────────────────────────────────────────────────────────────────────────
// DataFrame → Burn TensorData (직접 변환 경로)
// ─────────────────────────────────────────────────────────────────────────────

/// 지정한 숫자형 컬럼들을 [n_rows, n_cols] 형태의 Burn `TensorData` 로 변환한다.
///
/// 검증 실패(비숫자 컬럼 포함, 존재하지 않는 컬럼)는 Err 로 즉시 보고한다.
/// 반환된 TensorData 는 `Tensor::<B, 2>::from_data(data, &device)` 로
/// 어느 백엔드에서든 무오류로 소비 가능하다.
pub fn df_to_tensor_data(df: &DataFrame, cols: &[String]) -> Result<TensorData, String> {
    if cols.is_empty() {
        return Err("텐서 변환 에러: 변환할 컬럼이 지정되지 않았습니다.".into());
    }
    let n = df.height();
    let d = cols.len();

    let mut col_vecs: Vec<Vec<f32>> = Vec::with_capacity(d);
    for name in cols {
        let col = df
            .column(name.as_str())
            .map_err(|e| format!("텐서 변환 에러: 컬럼 '{name}' 접근 실패 — {e}"))?;
        if !is_numeric_dtype(col.dtype()) {
            return Err(format!(
                "텐서 변환 에러: 컬럼 '{name}' 는 숫자형이 아닙니다 (실제: {:?}). \
                 cast(\"{name}\", \"float\") 로 먼저 변환하세요.",
                col.dtype()
            ));
        }
        col_vecs.push(series_to_f32(col));
    }

    // 행 우선(row-major) 평탄화: [n, d]
    let mut xs = Vec::with_capacity(n * d);
    for i in 0..n {
        for cv in &col_vecs {
            xs.push(cv[i]);
        }
    }

    Ok(TensorData::new(xs, [n, d]))
}

/// DataFrame 의 모든 숫자형 컬럼을 자동 선택하여 TensorData 로 변환한다.
/// 반환: (선택된 컬럼명 순서, TensorData [n_rows, n_numeric_cols])
pub fn df_to_tensor_data_auto(df: &DataFrame) -> Result<(Vec<String>, TensorData), String> {
    let cols: Vec<String> = df
        .get_column_names()
        .iter()
        .filter(|name| {
            df.column(name.as_str())
                .map(|c| is_numeric_dtype(c.dtype()))
                .unwrap_or(false)
        })
        .map(|s| s.to_string())
        .collect();
    if cols.is_empty() {
        return Err("텐서 변환 에러: 숫자형 컬럼이 하나도 없습니다.".into());
    }
    let data = df_to_tensor_data(df, &cols)?;
    Ok((cols, data))
}

// ─────────────────────────────────────────────────────────────────────────────
// 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use burn::tensor::Tensor;
    use burn_ndarray::NdArray;
    use polars::prelude::df;

    type B = NdArray<f32>;

    #[test]
    fn df_to_tensor_data_shape_and_values() {
        let frame = df!(
            "a" => [1.0f64, 2.0, 3.0],
            "b" => [10i64, 20, 30],
        )
        .unwrap();
        let data = df_to_tensor_data(&frame, &["a".into(), "b".into()]).unwrap();
        assert_eq!(data.shape, [3, 2].into());
        let vals = data.to_vec::<f32>().unwrap();
        assert_eq!(vals, vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]);
    }

    #[test]
    fn tensor_data_feeds_burn_tensor_without_error() {
        // DoD: 전처리된 데이터가 Burn 텐서 연산 계층으로 무오류 변환됨
        let frame = df!(
            "age" => [65.0f64, 72.0, 88.0, 45.0],
            "cost" => [1200i64, 3400, 8800, 500],
        )
        .unwrap();
        let (cols, data) = df_to_tensor_data_auto(&frame).unwrap();
        assert_eq!(cols, vec!["age", "cost"]);

        let device = Default::default();
        let t = Tensor::<B, 2>::from_data(data, &device);
        assert_eq!(t.shape().dims::<2>(), [4, 2]);
        // 텐서 연산까지 무오류 확인
        let doubled = t.clone() + t;
        assert_eq!(doubled.shape().dims::<2>(), [4, 2]);
    }

    #[test]
    fn non_numeric_column_is_rejected_with_hint() {
        let frame = df!(
            "region" => ["SEOUL", "BUSAN"],
            "x" => [1.0f64, 2.0],
        )
        .unwrap();
        let err = df_to_tensor_data(&frame, &["region".into()]).unwrap_err();
        assert!(err.contains("숫자형이 아닙니다"));
    }

    #[test]
    fn null_becomes_nan() {
        let frame = df!("x" => [Some(1.0f64), None]).unwrap();
        let data = df_to_tensor_data(&frame, &["x".into()]).unwrap();
        let vals = data.to_vec::<f32>().unwrap();
        assert_eq!(vals[0], 1.0);
        assert!(vals[1].is_nan());
    }

    #[test]
    fn extract_data_separates_features_and_target() {
        let frame = df!(
            "region" => ["a", "b"],          // 비숫자 → 특성 제외
            "age" => [65.0f64, 72.0],
            "cost" => [1200.0f64, 3400.0],   // 타겟
        )
        .unwrap();
        let (names, features, targets) = extract_data(&frame, "cost").unwrap();
        assert_eq!(names, vec!["age"]);
        assert_eq!(features, vec![vec![65.0f32], vec![72.0f32]]);
        assert_eq!(targets, vec![1200.0f32, 3400.0f32]);
    }

    #[test]
    fn extract_data_missing_target_lists_candidates() {
        let frame = df!("age" => [1.0f64]).unwrap();
        let err = extract_data(&frame, "cost").unwrap_err();
        assert!(err.contains("cost"));
        assert!(err.contains("age"));
    }
}
