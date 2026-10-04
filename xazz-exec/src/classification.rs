//! Classification reports for finite integer labels and raw model logits.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClassificationMetrics {
    pub num_classes: usize,
    pub accuracy: f64,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    /// Positive-class probability ROC AUC; unavailable for multiclass or a
    /// partition containing only one of the two classes.
    pub auc: Option<f64>,
    /// Row-major: actual class on rows, predicted class on columns.
    pub confusion_matrix: Vec<usize>,
}

pub fn detect_classification(targets: &[f32], max_classes: usize) -> Option<Vec<f32>> {
    let mut classes = Vec::new();
    for &label in targets {
        if !label.is_finite() || label.fract() != 0.0 {
            return None;
        }
        if !classes.contains(&label) {
            classes.push(label);
            if classes.len() > max_classes {
                return None;
            }
        }
    }
    if classes.len() < 2 {
        return None;
    }
    classes.sort_by(f32::total_cmp);
    Some(classes)
}

pub fn classification_metrics(
    preds: &[f32],
    targets: &[f32],
    classes: &[f32],
    positive_scores: Option<&[f32]>,
) -> Result<ClassificationMetrics, String> {
    let k = classes.len();
    if k < 2
        || targets.is_empty()
        || preds.len() != targets.len()
        || classes
            .iter()
            .enumerate()
            .any(|(i, c)| !c.is_finite() || classes[..i].contains(c))
    {
        return Err(
            "classification metrics require matching nonempty rows and distinct finite classes"
                .into(),
        );
    }
    let mut cm = vec![0usize; k * k];
    for (&pred, &actual) in preds.iter().zip(targets) {
        let a = classes
            .iter()
            .position(|&c| c == actual)
            .ok_or("unknown target class")?;
        let p = classes
            .iter()
            .position(|&c| c == pred)
            .ok_or("unknown predicted class")?;
        cm[a * k + p] += 1;
    }
    let (mut precision, mut recall, mut f1) = (0.0, 0.0, 0.0);
    let mut correct = 0;
    for c in 0..k {
        let tp = cm[c * k + c];
        correct += tp;
        let predicted: usize = (0..k).map(|a| cm[a * k + c]).sum();
        let actual: usize = cm[c * k..(c + 1) * k].iter().sum();
        let p = if predicted == 0 {
            0.0
        } else {
            tp as f64 / predicted as f64
        };
        let r = if actual == 0 {
            0.0
        } else {
            tp as f64 / actual as f64
        };
        precision += p;
        recall += r;
        if p + r > 0.0 {
            f1 += 2.0 * p * r / (p + r);
        }
    }
    let auc = if let Some(scores) = positive_scores {
        if scores.len() != targets.len()
            || scores
                .iter()
                .any(|s| !s.is_finite() || !(0.0..=1.0).contains(s))
        {
            return Err("AUC requires one finite probability per target".into());
        }
        if k == 2 {
            binary_auc(scores, targets, classes[1])
        } else {
            None
        }
    } else {
        None
    };
    Ok(ClassificationMetrics {
        num_classes: k,
        accuracy: correct as f64 / targets.len() as f64,
        precision: precision / k as f64,
        recall: recall / k as f64,
        f1: f1 / k as f64,
        auc,
        confusion_matrix: cm,
    })
}

fn binary_auc(scores: &[f32], targets: &[f32], positive: f32) -> Option<f64> {
    let pos = targets.iter().filter(|&&y| y == positive).count();
    let neg = targets.len() - pos;
    if pos == 0 || neg == 0 {
        return None;
    }
    let mut rows: Vec<_> = scores
        .iter()
        .copied()
        .zip(targets.iter().map(|&y| y == positive))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut rank_sum, mut start) = (0.0, 0);
    while start < rows.len() {
        let mut end = start + 1;
        while end < rows.len() && rows[end].0 == rows[start].0 {
            end += 1;
        }
        let rank = (start + 1 + end) as f64 / 2.0;
        rank_sum += rank * rows[start..end].iter().filter(|r| r.1).count() as f64;
        start = end;
    }
    Some((rank_sum - pos as f64 * (pos + 1) as f64 / 2.0) / (pos as f64 * neg as f64))
}

pub fn decode_logits(
    logits: &[f32],
    classes: &[f32],
) -> Result<(Vec<f32>, Option<Vec<f32>>), String> {
    let k = classes.len();
    if k < 2 || !logits.len().is_multiple_of(k) || logits.iter().any(|v| !v.is_finite()) {
        return Err("classification output must contain finite logits for every class".into());
    }
    let mut labels = Vec::with_capacity(logits.len() / k);
    let mut scores = (k == 2).then(Vec::new);
    for row in logits.chunks_exact(k) {
        let best = (0..k)
            .max_by(|&a, &b| row[a].total_cmp(&row[b]).then_with(|| b.cmp(&a)))
            .unwrap();
        labels.push(classes[best]);
        if let Some(scores) = &mut scores {
            let max = row[best];
            let sum: f32 = row.iter().map(|x| (x - max).exp()).sum();
            scores.push((row[1] - max).exp() / sum);
        }
    }
    Ok((labels, scores))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhaustive_auc_matches_independent_pairwise_definition() {
        for mask in 0..16 {
            let targets: Vec<_> = (0..4).map(|i| ((mask >> i) & 1) as f32).collect();
            for encoded in 0..256 {
                let scores: Vec<_> = (0..4)
                    .map(|i| ((encoded >> (i * 2)) & 3) as f32 / 3.0)
                    .collect();
                let predictions: Vec<_> = scores
                    .iter()
                    .map(|&p| if p >= 0.5 { 1. } else { 0. })
                    .collect();
                let actual =
                    classification_metrics(&predictions, &targets, &[0., 1.], Some(&scores))
                        .unwrap();
                let (mut wins, mut pairs) = (0.0, 0usize);
                for positive in (0..4).filter(|&i| targets[i] == 1.) {
                    for negative in (0..4).filter(|&i| targets[i] == 0.) {
                        pairs += 1;
                        wins += if scores[positive] > scores[negative] {
                            1.0
                        } else if scores[positive] == scores[negative] {
                            0.5
                        } else {
                            0.0
                        };
                    }
                }
                let expected = (pairs > 0).then(|| wins / pairs as f64);
                assert_eq!(
                    actual.auc, expected,
                    "targets={targets:?}, scores={scores:?}"
                );
            }
        }
    }

    #[test]
    fn exhaustive_macro_metrics_match_direct_label_counts() {
        for k in 2usize..=4 {
            let classes: Vec<_> = (0..k).map(|c| c as f32).collect();
            for encoding in 0..k.pow(8) {
                let mut value = encoding;
                let labels: Vec<_> = (0..8)
                    .map(|_| {
                        let c = (value % k) as f32;
                        value /= k;
                        c
                    })
                    .collect();
                let (predictions, targets) = labels.split_at(4);
                let actual = classification_metrics(predictions, targets, &classes, None).unwrap();
                let (mut p, mut r, mut f) = (0.0, 0.0, 0.0);
                for &class in &classes {
                    let tp = predictions
                        .iter()
                        .zip(targets)
                        .filter(|(a, b)| **a == class && **b == class)
                        .count() as f64;
                    let guessed = predictions.iter().filter(|&&c| c == class).count() as f64;
                    let present = targets.iter().filter(|&&c| c == class).count() as f64;
                    if guessed > 0.0 {
                        p += tp / guessed;
                    }
                    if present > 0.0 {
                        r += tp / present;
                    }
                    if guessed + present > 0.0 {
                        f += 2.0 * tp / (guessed + present);
                    }
                }
                assert!((actual.precision - p / k as f64).abs() < 1e-12);
                assert!((actual.recall - r / k as f64).abs() < 1e-12);
                assert!((actual.f1 - f / k as f64).abs() < 1e-12);
                assert_eq!(
                    actual.accuracy,
                    predictions
                        .iter()
                        .zip(targets)
                        .filter(|(a, b)| a == b)
                        .count() as f64
                        / 4.0
                );
            }
        }
    }

    #[test]
    fn macro_metrics_and_confusion_are_exact() {
        let m = classification_metrics(
            &[0., 1., 1., 1., 2., 0.],
            &[0., 0., 1., 1., 2., 2.],
            &[0., 1., 2.],
            None,
        )
        .unwrap();
        assert_eq!(m.confusion_matrix, vec![1, 1, 0, 0, 2, 0, 1, 0, 1]);
        assert!((m.accuracy - 2.0 / 3.0).abs() < 1e-12);
        assert!((m.precision - (0.5 + 2.0 / 3.0 + 1.0) / 3.0).abs() < 1e-12);
        assert!((m.recall - 2.0 / 3.0).abs() < 1e-12);
        assert!((m.f1 - (0.5 + 0.8 + 2.0 / 3.0) / 3.0).abs() < 1e-12);
        assert!(m.auc.is_none());
    }
    #[test]
    fn auc_uses_probabilities_and_average_ranks_for_ties() {
        let y = [0., 0., 1., 1.];
        let m = classification_metrics(
            &[0., 1., 0., 1.],
            &y,
            &[0., 1.],
            Some(&[0.1, 0.4, 0.35, 0.8]),
        )
        .unwrap();
        assert_eq!(m.auc, Some(0.75));
        assert_eq!(binary_auc(&[0.5; 4], &y, 1.), Some(0.5));
        assert_eq!(binary_auc(&[0.1, 0.2], &[0., 0.], 1.), None);
    }
    #[test]
    fn missing_classes_zero_denominators_and_json_are_explicit() {
        let m = classification_metrics(&[0., 0.], &[0., 0.], &[0., 1.], Some(&[0.1, 0.2])).unwrap();
        assert_eq!(m.precision, 0.5);
        assert_eq!(m.recall, 0.5);
        assert_eq!(m.f1, 0.5);
        assert!(serde_json::to_value(m).unwrap()["auc"].is_null());
        assert!(classification_metrics(&[], &[], &[0., 1.], None).is_err());
        assert!(classification_metrics(&[2.], &[0.], &[0., 1.], None).is_err());
        assert!(detect_classification(&[0., f32::NAN, 1.], 10).is_none());
    }
    #[test]
    fn logits_decode_to_original_labels_without_overflow() {
        let (labels, scores) = decode_logits(&[1000., 999., -1000., -999.], &[-3., 7.]).unwrap();
        assert_eq!(labels, vec![-3., 7.]);
        let scores = scores.unwrap();
        assert!(scores[0] < 0.5 && scores[1] > 0.5);
        assert!(decode_logits(&[f32::NAN, 0.], &[0., 1.]).is_err());
    }
}
