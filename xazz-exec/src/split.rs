//! Deterministic validation partitions fitted before training preprocessing.
use std::collections::BTreeMap;

use polars::prelude::{AnyValue, DataFrame, DataType};
use xazz_core::ast::{SplitStrategy, TrainConfig};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SplitReport {
    pub strategy: SplitStrategy,
    pub time_column: Option<String>,
    pub requested_fraction: f64,
    pub train_rows: usize,
    pub validation_rows: usize,
    pub classes: Vec<ClassCount>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClassCount {
    pub label: f32,
    pub train_rows: usize,
    pub validation_rows: usize,
}

impl SplitReport {
    pub fn new(config: &TrainConfig, train: &[usize], val: &[usize], targets: &[f32]) -> Self {
        let mut classes = BTreeMap::<u32, ClassCount>::new();
        if config.split_strategy == SplitStrategy::Stratified {
            for (indices, validation) in [(train, false), (val, true)] {
                for &i in indices {
                    let label = targets[i];
                    let entry = classes.entry(class_key(label)).or_insert(ClassCount {
                        label,
                        train_rows: 0,
                        validation_rows: 0,
                    });
                    if validation {
                        entry.validation_rows += 1;
                    } else {
                        entry.train_rows += 1;
                    }
                }
            }
        }
        Self {
            strategy: config.split_strategy,
            time_column: config.time_column.as_deref().map(str::to_owned),
            requested_fraction: config.validation_split.unwrap_or(0.0),
            train_rows: train.len(),
            validation_rows: val.len(),
            classes: classes.into_values().collect(),
        }
    }
}

fn class_key(value: f32) -> u32 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

fn times(df: &DataFrame, name: &str) -> Result<Vec<AnyValue<'static>>, String> {
    let column = df
        .column(name)
        .map_err(|_| format!("time_column '{name}' not found"))?;
    if !column.dtype().is_numeric() {
        return Err(format!("time_column '{name}' must be numeric"));
    }
    let cast = column.cast(&DataType::Float64).map_err(|e| e.to_string())?;
    if cast
        .f64()
        .map_err(|e| e.to_string())?
        .into_iter()
        .any(|value| value.is_none_or(|v| !v.is_finite()))
    {
        return Err(format!(
            "time_column '{name}' contains null or non-finite values"
        ));
    }
    // 원래 정수 타입을 보존해 2^53보다 큰 타임스탬프도 정확하게 정렬한다.
    (0..column.len())
        .map(|i| {
            column
                .get(i)
                .map(|value| match value {
                    AnyValue::Float32(0.0) => AnyValue::Float32(0.0),
                    AnyValue::Float64(0.0) => AnyValue::Float64(0.0),
                    value => value.into_static(),
                })
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub(crate) fn time_order_by_column(df: &DataFrame, name: &str) -> Result<Vec<usize>, String> {
    let values = times(df, name)?;
    let mut order: Vec<_> = (0..values.len()).collect();
    order.sort_by(|&a, &b| {
        values[a]
            .partial_cmp(&values[b])
            .expect("finite homogeneous numeric column")
    });
    Ok(order)
}

pub(crate) fn validate_time_boundary(
    df: &DataFrame,
    name: &str,
    train: &[usize],
    val: &[usize],
) -> Result<(), String> {
    let values = times(df, name)?;
    if let (Some(&last), Some(&first)) = (train.last(), val.first())
        && values[last] >= values[first]
    {
        return Err("time split boundary must be strictly increasing; choose a validation_split that does not divide equal timestamps".into());
    }
    Ok(())
}

/// Single-target stratification. Keep every class in training; singleton
/// classes never enter validation. Cover each non-singleton class in validation
/// when the requested size allows it, then apportion remaining rows by deficit.
/// Time ordering is only valid with a sequential split.
pub fn select_split_indices(
    n: usize,
    fraction: f64,
    strategy: SplitStrategy,
    targets: &[f32],
    time_order: Option<&[usize]>,
) -> Result<(Vec<usize>, Vec<usize>), String> {
    if !fraction.is_finite() || !(0.0..=0.9).contains(&fraction) {
        return Err("validation_split must be finite and between 0 and 0.9".into());
    }
    if time_order.is_some() && strategy != SplitStrategy::Sequential {
        return Err(
            "time_column requires split: sequential; random/stratified can leak future rows".into(),
        );
    }
    if strategy == SplitStrategy::Stratified
        && (targets.len() != n || targets.iter().any(|v| !v.is_finite() || v.fract() != 0.0))
    {
        return Err("stratified split requires one finite integer class label per row".into());
    }
    let mut order: Vec<_> = time_order.map_or_else(|| (0..n).collect(), |v| v.to_vec());
    let mut sorted = order.clone();
    sorted.sort_unstable();
    if sorted != (0..n).collect::<Vec<_>>() {
        return Err("time order must be a permutation of the input rows".into());
    }
    let val_n = (n as f64 * fraction) as usize;
    if fraction == 0.0 {
        return Ok((order, Vec::new()));
    }
    if val_n == 0 || val_n >= n {
        return Err("validation_split must leave at least one row in each partition".into());
    }
    match strategy {
        SplitStrategy::Sequential => {}
        SplitStrategy::Random => {
            let mut seed = 0x9E37_79B9_7F4A_7C15u64;
            for i in (1..n).rev() {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                order.swap(i, (seed >> 33) as usize % (i + 1));
            }
        }
        SplitStrategy::Stratified => {
            let mut groups = BTreeMap::<u32, Vec<usize>>::new();
            for &i in &order {
                groups.entry(class_key(targets[i])).or_default().push(i);
            }
            let groups: Vec<_> = groups.into_values().collect();
            if val_n > n - groups.len() {
                return Err(
                    "validation_split is too large to retain every class in training".into(),
                );
            }
            let eligible = groups.iter().filter(|g| g.len() > 1).count();
            let cover_all = val_n >= eligible;
            let mut counts: Vec<usize> = groups
                .iter()
                .map(|g| usize::from(cover_all && g.len() > 1))
                .collect();
            for _ in counts.iter().sum::<usize>()..val_n {
                let best = (0..groups.len())
                    .filter(|&i| counts[i] < groups[i].len() - 1)
                    .max_by(|&a, &b| {
                        let deficit =
                            |i: usize| groups[i].len() as f64 * fraction - counts[i] as f64;
                        deficit(a).total_cmp(&deficit(b)).then_with(|| b.cmp(&a))
                    })
                    .ok_or("cannot allocate validation rows while retaining classes")?;
                counts[best] += 1;
            }
            let mut is_val = vec![false; n];
            for (group, count) in groups.iter().zip(counts) {
                for &i in &group[group.len() - count..] {
                    is_val[i] = true;
                }
            }
            return Ok(order.into_iter().partition(|&i| !is_val[i]));
        }
    }
    let val = order.split_off(n - val_n);
    Ok((order, val))
}

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::*;

    #[test]
    fn zero_fraction_stratified_checks_labels_before_returning() {
        assert_eq!(
            select_split_indices(4, 0.0, SplitStrategy::Stratified, &[0., 1., 0., 1.], None)
                .unwrap(),
            (vec![0, 1, 2, 3], Vec::new())
        );
        for targets in [
            vec![0., 1.],
            vec![0., 1., f32::NAN, 1.],
            vec![0., 1., f32::INFINITY, 1.],
            vec![0., 1., 0.5, 1.],
        ] {
            let error = select_split_indices(4, 0.0, SplitStrategy::Stratified, &targets, None)
                .expect_err("invalid class labels must fail even without validation rows");
            assert!(error.contains("finite integer class label"), "{error}");
        }
        for strategy in [SplitStrategy::Sequential, SplitStrategy::Random] {
            assert!(select_split_indices(4, 0.0, strategy, &[0.5], None).is_ok());
        }
    }

    #[test]
    fn integer_timestamps_preserve_precision_above_float64_exact_range() {
        let base = 1u64 << 53;
        let frame = df!("time" => [base+1, base, base+3, base+2]).unwrap();
        let order = time_order_by_column(&frame, "time").unwrap();
        assert_eq!(order, vec![1, 0, 3, 2]);
        validate_time_boundary(&frame, "time", &order[..1], &order[1..]).unwrap();
        let zeros = df!("time" => [-0.0f64, 0.0f64]).unwrap();
        assert!(validate_time_boundary(&zeros, "time", &[0], &[1]).is_err());
    }

    #[test]
    fn exhaustive_small_partitions_obey_capacity_and_coverage() {
        for n in 1usize..=7 {
            for encoding in 0..3usize.pow(n as u32) {
                let mut encoded = encoding;
                let labels: Vec<f32> = (0..n)
                    .map(|_| {
                        let label = (encoded % 3) as f32;
                        encoded /= 3;
                        label
                    })
                    .collect();
                let sizes: Vec<usize> = (0..3)
                    .map(|c| labels.iter().filter(|&&y| y == c as f32).count())
                    .collect();
                for tenth in 1..=9 {
                    let fraction = tenth as f64 / 10.0;
                    let want = (n as f64 * fraction).floor() as usize;
                    let capacity: usize = sizes.iter().map(|&count| count.saturating_sub(1)).sum();
                    for strategy in [
                        SplitStrategy::Sequential,
                        SplitStrategy::Random,
                        SplitStrategy::Stratified,
                    ] {
                        let result = select_split_indices(n, fraction, strategy, &labels, None);
                        let feasible = want > 0
                            && want < n
                            && (strategy != SplitStrategy::Stratified || want <= capacity);
                        assert_eq!(
                            result.is_ok(),
                            feasible,
                            "n={n}, labels={labels:?}, fraction={fraction}, strategy={strategy:?}"
                        );
                        if let Ok((train, val)) = result {
                            assert_eq!(val.len(), want);
                            let mut combined: Vec<_> = train.iter().chain(&val).copied().collect();
                            combined.sort_unstable();
                            assert_eq!(combined, (0..n).collect::<Vec<_>>());
                            if strategy == SplitStrategy::Stratified {
                                let eligible = sizes.iter().filter(|&&size| size > 1).count();
                                for (class, &size) in
                                    sizes.iter().enumerate().filter(|(_, size)| **size > 0)
                                {
                                    assert!(train.iter().any(|&i| labels[i] == class as f32));
                                    if size == 1 {
                                        assert!(!val.iter().any(|&i| labels[i] == class as f32));
                                    }
                                    if size > 1 && want >= eligible {
                                        assert!(val.iter().any(|&i| labels[i] == class as f32));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn partitions_are_disjoint_complete_and_reproducible() {
        for strategy in [
            SplitStrategy::Sequential,
            SplitStrategy::Random,
            SplitStrategy::Stratified,
        ] {
            let y: Vec<_> = (0..20).map(|i| (i % 3) as f32).collect();
            let a = select_split_indices(20, 0.25, strategy, &y, None).unwrap();
            assert_eq!(
                a,
                select_split_indices(20, 0.25, strategy, &y, None).unwrap()
            );
            assert_eq!((a.0.len(), a.1.len()), (15, 5));
            let mut all: Vec<_> = a.0.into_iter().chain(a.1).collect();
            all.sort_unstable();
            assert_eq!(all, (0..20).collect::<Vec<_>>());
        }
    }
    #[test]
    fn rare_classes_stay_in_training_and_validation_size_is_exact() {
        let y = [0., 0., 0., 0., 1., 1., 2.];
        let (tr, va) = select_split_indices(7, 0.5, SplitStrategy::Stratified, &y, None).unwrap();
        assert_eq!(va.len(), 3);
        assert!(tr.contains(&6));
        for label in [0., 1.] {
            assert!(tr.iter().any(|&i| y[i] == label));
            assert!(va.iter().any(|&i| y[i] == label));
        }
        assert!(select_split_indices(7, 0.9, SplitStrategy::Stratified, &y, None).is_err());
    }
    #[test]
    fn small_validation_does_not_force_one_per_class() {
        let y = [0., 0., 1., 1., 2., 2.];
        let (tr, va) = select_split_indices(6, 0.2, SplitStrategy::Stratified, &y, None).unwrap();
        assert_eq!(va.len(), 1);
        assert_eq!(tr.len(), 5);
    }
    #[test]
    fn invalid_labels_orders_and_time_combinations_fail_closed() {
        for y in [vec![0., f32::NAN], vec![0., 0.5], vec![0.]] {
            assert!(select_split_indices(2, 0.5, SplitStrategy::Stratified, &y, None).is_err());
        }
        for strategy in [SplitStrategy::Random, SplitStrategy::Stratified] {
            assert!(select_split_indices(2, 0.5, strategy, &[0., 1.], Some(&[0, 1])).is_err());
        }
        assert!(
            select_split_indices(2, 0.5, SplitStrategy::Sequential, &[], Some(&[0, 0])).is_err()
        );
    }
    #[test]
    fn temporal_partition_is_strict_and_rejects_missing_or_tied_times() {
        let df = df!("t" => [4., 1., 3., 2.]).unwrap();
        let order = time_order_by_column(&df, "t").unwrap();
        let (tr, va) =
            select_split_indices(4, 0.5, SplitStrategy::Sequential, &[], Some(&order)).unwrap();
        assert_eq!(tr, vec![1, 3]);
        assert_eq!(va, vec![2, 0]);
        validate_time_boundary(&df, "t", &tr, &va).unwrap();
        let tied = df!("t" => [1., 2., 2., 3.]).unwrap();
        assert!(validate_time_boundary(&tied, "t", &[0, 1], &[2, 3]).is_err());
        let missing = df!("t" => [Some(1.), None]).unwrap();
        assert!(time_order_by_column(&missing, "t").is_err());
    }
}
