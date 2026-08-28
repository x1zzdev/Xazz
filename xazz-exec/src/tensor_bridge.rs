//! xazz-exec/src/tensor_bridge.rs — Polars ↔ Burn 데이터 변환 인터페이스 (v0.7)
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
//!
//! ## 메모리 모델 (복사 경계)
//!
//! v0.7 부터 **연속(단일 청크·무결측) Float32/Float64 컬럼**은 Arrow 버퍼를
//! `cont_slice()` 로 직접 읽어 f64→f32 변환만 수행한다 (별도 캐스팅 복사 제거).
//! 불가피한 복사 경계:
//!   - f64 → f32 정밀도 강등 (Burn CPU 백엔드가 f32 이므로 필수)
//!   - columnar → row-major [n, d] 재배치 (Burn `TensorData` 가 owned Vec 요구)
//!   - host → device (Burn `Tensor::from_data` 의 책임)

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
///
/// 연속(무결측·단일 청크) Float64/Float32 컬럼은 Arrow raw 버퍼를 직접 읽는다.
pub fn series_to_f32(col: &Column) -> Vec<f32> {
    // 빠른 경로 1: Float64 연속 → raw f64 버퍼를 f32 로 강등 (캐스팅 복사 제거)
    if let Ok(ca) = col.f64()
        && let Ok(slice) = ca.cont_slice()
    {
        return slice.iter().map(|&v| v as f32).collect();
    }
    // 빠른 경로 2: Float32 연속 → raw f32 버퍼 복사 (변환 없음)
    if let Ok(ca) = col.f32()
        && let Ok(slice) = ca.cont_slice()
    {
        return slice.to_vec();
    }

    // 일반 경로: 개별 dtype 분기 대신 Float64 캐스팅 경로로 일원화.
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
///
/// 메모리: 연속 Float32/Float64 컬럼은 Arrow raw 버퍼를 직접 읽어
/// row-major 벡터 한 번에 채운다 (컬럼별 중간 Vec 복사 없음).
pub fn df_to_tensor_data(df: &DataFrame, cols: &[String]) -> Result<TensorData, String> {
    if cols.is_empty() {
        return Err("텐서 변환 에러: 변환할 컬럼이 지정되지 않았습니다.".into());
    }
    let n = df.height();
    let d = cols.len();

    // 컬럼별 원본 참조: 연속 f32/f64 는 무복사 슬라이스, 그 외(정수/결측)는 소유 Vec.
    let mut srcs: Vec<ColSrc> = Vec::with_capacity(d);
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
        let mut src: Option<ColSrc> = None;
        if let Ok(ca) = col.f32()
            && let Ok(slice) = ca.cont_slice()
        {
            src = Some(ColSrc::F32(slice));
        }
        if src.is_none()
            && let Ok(ca) = col.f64()
            && let Ok(slice) = ca.cont_slice()
        {
            src = Some(ColSrc::F64(slice));
        }
        srcs.push(src.unwrap_or_else(|| ColSrc::Owned(series_to_f32(col))));
    }

    // 행 우선(row-major) 평탄화: [n, d] — 단일 사전할당 버퍼에 한 번에 채운다.
    let mut xs = Vec::with_capacity(n * d);
    for i in 0..n {
        for src in &srcs {
            match src {
                ColSrc::F32(s) => xs.push(s[i]),
                ColSrc::F64(s) => xs.push(s[i] as f32),
                ColSrc::Owned(v) => xs.push(v[i]),
            }
        }
    }

    Ok(TensorData::new(xs, [n, d]))
}

/// 컬럼 원본: 무복사 Arrow 슬라이스 또는 (정수/결측 컬럼용) 소유 벡터.
enum ColSrc<'a> {
    F32(&'a [f32]),
    F64(&'a [f64]),
    Owned(Vec<f32>),
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
    fn f32_and_f64_contiguous_columns_read_directly() {
        // f32/f64 연속 컬럼은 raw 버퍼를 직접 읽고 [n, d] row-major 로 배치
        let frame = df!(
            "a" => [1.0f32, 2.0, 3.0],
            "b" => [10.0f64, 20.0, 30.0],
        )
        .unwrap();
        let data = df_to_tensor_data(&frame, &["a".into(), "b".into()]).unwrap();
        assert_eq!(data.shape, [3, 2].into());
        let vals = data.to_vec::<f32>().unwrap();
        assert_eq!(vals, vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]);
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
