//! xazz-exec/src/tensor_bridge.rs — Polars ↔ Burn data conversion interface (v0.7)
//!
//! Official boundary between the data acceleration layer (Polars DataFrame) and
//! the deep-learning compilation layer (Burn Tensor). All DataFrame → Tensor
//! conversions on the training/inference path go through this module.
//!
//! ## dtype normalization contract (Polars → tensor)
//!
//! | Polars DataType                | tensor element | missing (null) handling |
//! |--------------------------------|----------------|--------------------------|
//! | Float64 / Float32              | f32            | passed as NaN            |
//! | Int8~64 / UInt8~64             | f32            | passed as NaN            |
//! | String / Bool / Date, etc. non-numeric | excluded | —                        |
//!
//! Why missing values are passed as NaN: the imputation strategy (mean, etc.)
//! is the caller's responsibility (dl::train normalization step); the bridge
//! passes values through without distortion.
//!
//! ## Memory model (copy boundary)
//!
//! Since v0.7, **contiguous (single-chunk, missing-free) Float32/Float64 columns**
//! are read directly from the Arrow buffer via `cont_slice()`, performing only
//! the f64→f32 conversion (eliminating a separate casting copy).
//! Unavoidable copy boundaries:
//!   - f64 → f32 precision downgrade (required because Burn's CPU backend is f32)
//!   - columnar → row-major [n, d] rearrangement (Burn `TensorData` requires an owned Vec)
//!   - host → device (Burn `Tensor::from_data`'s responsibility)

use burn::tensor::TensorData;
use polars::prelude::{Column, DataFrame, DataType};

// ─────────────────────────────────────────────────────────────────────────────
// dtype normalization
// ─────────────────────────────────────────────────────────────────────────────

/// Determines whether the dtype is a numeric type eligible for tensor conversion.
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

/// Converts column values to an f32 vector. null → NaN. (see dtype normalization contract)
///
/// Contiguous (missing-free, single-chunk) Float64/Float32 columns read the Arrow raw buffer directly.
pub fn series_to_f32(col: &Column) -> Vec<f32> {
    // Fast path 1: Float64 contiguous → downgrade the raw f64 buffer to f32 (no casting copy)
    if let Ok(ca) = col.f64()
        && let Ok(slice) = ca.cont_slice()
    {
        return slice.iter().map(|&v| v as f32).collect();
    }
    // Fast path 2: Float32 contiguous → copy the raw f32 buffer (no conversion)
    if let Ok(ca) = col.f32()
        && let Ok(slice) = ca.cont_slice()
    {
        return slice.to_vec();
    }

    // General path: unify via the Float64 casting path instead of per-dtype branching.
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
// feature/target extraction (training path)
// ─────────────────────────────────────────────────────────────────────────────

/// Extracts numeric columns (excluding the target) as features and the target column as the label.
///
/// Returns: (feature column-name order, per-row feature values, target values)
/// The column-name order must be preserved, as it maps 1:1 to later normalization
/// statistics and prediction inputs.
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
// DataFrame → Burn TensorData (direct conversion path)
// ─────────────────────────────────────────────────────────────────────────────

/// Converts the specified numeric columns to a Burn `TensorData` shaped [n_rows, n_cols].
///
/// Validation failures (non-numeric column, missing column) are reported immediately as Err.
/// The returned TensorData can be consumed without error on any backend via
/// `Tensor::<B, 2>::from_data(data, &device)`.
///
/// Memory: contiguous Float32/Float64 columns are read directly from the Arrow raw buffer
/// and filled into a single row-major vector (no per-column intermediate Vec copies).
pub fn df_to_tensor_data(df: &DataFrame, cols: &[String]) -> Result<TensorData, String> {
    if cols.is_empty() {
        return Err("텐서 변환 에러: 변환할 컬럼이 지정되지 않았습니다.".into());
    }
    let n = df.height();
    let d = cols.len();

    // Per-column source: contiguous f32/f64 use copy-free slices, others (int/missing) use owned Vecs.
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

    // Row-major flattening: [n, d] — fill a single preallocated buffer in one pass.
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

/// Column source: a copy-free Arrow slice or (for int/missing columns) an owned vector.
enum ColSrc<'a> {
    F32(&'a [f32]),
    F64(&'a [f64]),
    Owned(Vec<f32>),
}

/// Automatically selects all numeric columns in the DataFrame and converts them to TensorData.
/// Returns: (selected column-name order, TensorData [n_rows, n_numeric_cols])
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
// tests
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
        // DoD: preprocessed data converts to the Burn tensor op layer without error
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
        // verify tensor ops succeed without error
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
        // f32/f64 contiguous columns read the raw buffer directly and are laid out [n, d] row-major
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
            "region" => ["a", "b"],          // non-numeric → excluded from features
            "age" => [65.0f64, 72.0],
            "cost" => [1200.0f64, 3400.0],   // target
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
