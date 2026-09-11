// xazz-exec/src/sanitize.rs — fine-tuning data sanitization (issue #72, F3)
//
// "Safe data in" before LoRA/QLoRA fine-tuning reaches the engine. Reads any
// supported source (CSV/Parquet/Arrow) and produces a structured sanitization
// report with three checks:
//
//   1. PII scan        — cell-level re-scan using the same policy literal
//                        scanners as the guardrail (RRN, phone, email, card,
//                        API key, private key).
//   2. Duplicates      — exact duplicate-row rate + normalized-whitespace
//                        near-duplicate rate per text column.
//   3. Bias            — categorical columns (low cardinality) with a dominant
//                        category or a large max/min imbalance are flagged as a
//                        bias signal for fine-tuning data.
//
// The report is the fine-tune intake artifact: it records what was checked and
// what must be cleaned before training data reaches a fine-tuning engine.

use std::collections::HashMap;

use polars::prelude::{BooleanChunked, DataFrame};
use serde::Serialize;
use xazz_compiler::policy::patterns::{SecretKind, scan_output_text};

/// Sanitization report for one source file.
#[derive(Debug, Clone, Serialize)]
pub struct SanitizeReport {
    pub source: String,
    pub rows: usize,
    pub columns: usize,
    pub pii: PiiSection,
    pub duplicates: DuplicateSection,
    pub bias: BiasSection,
    /// Overall verdict — "sanitize" when any check flags the data.
    pub recommendation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct PiiSection {
    /// Columns containing PII/secret literals
    pub flagged_columns: Vec<PiiColumn>,
    pub total_findings: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PiiColumn {
    pub column: String,
    pub findings: usize,
    pub kinds: Vec<String>,
    /// Masked samples — raw values never appear in the report
    pub masked_samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateSection {
    /// Fraction of rows that are exact duplicates (0.0–1.0)
    pub exact_row_rate: f64,
    /// Text columns with a near-duplicate (normalized-whitespace) rate ≥ threshold
    pub near_duplicate_columns: Vec<NearDuplicateColumn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearDuplicateColumn {
    pub column: String,
    /// Fraction of non-null values that are near-duplicates after normalization
    pub near_duplicate_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BiasSection {
    /// Categorical columns flagged for imbalance
    pub flagged_columns: Vec<BiasColumn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BiasColumn {
    pub column: String,
    /// max count / min count among the present categories (≥1)
    pub imbalance_ratio: f64,
    /// Number of distinct categories
    pub cardinality: usize,
    /// Share of the most frequent category (0.0–1.0)
    pub dominant_share: f64,
    /// Top categories with counts
    pub top_categories: Vec<(String, u64)>,
}

/// Near-duplicate rate above this fraction flags a text column.
const NEAR_DUP_THRESHOLD: f64 = 0.2;
/// Columns with this many (or fewer) distinct categories are treated as categorical.
const MAX_CATEGORICAL_CARDINALITY: usize = 50;
/// Imbalance above this ratio flags a bias signal.
const BIAS_IMBALANCE_THRESHOLD: f64 = 10.0;
/// A dominant category holding ≥ this share is a bias signal.
const BIAS_DOMINANT_SHARE: f64 = 0.5;
/// Cap on masked samples kept per column in the report.
const MAX_MASKED_SAMPLES: usize = 3;
/// Cap on top categories reported per column.
const MAX_TOP_CATEGORIES: usize = 5;

/// Runs the sanitization checks on a source file.
pub fn sanitize_file(path: &str) -> Result<SanitizeReport, String> {
    let df = crate::runtime::load_source_as_df(path)
        .map_err(|e| format!("failed to read '{}': {e}", path))?;
    Ok(sanitize_df(&df, path))
}

/// Runs the sanitization checks on an already-loaded DataFrame.
pub fn sanitize_df(df: &DataFrame, source: &str) -> SanitizeReport {
    let col_names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let pii = scan_pii(df, &col_names);
    let duplicates = scan_duplicates(df, &col_names);
    let bias = scan_bias(df, &col_names);

    let needs_sanitize = !pii.flagged_columns.is_empty()
        || duplicates.exact_row_rate > 0.0
        || !duplicates.near_duplicate_columns.is_empty()
        || !bias.flagged_columns.is_empty();

    SanitizeReport {
        source: source.to_string(),
        rows: df.height(),
        columns: col_names.len(),
        pii,
        duplicates,
        bias,
        recommendation: if needs_sanitize {
            "sanitize — clean PII, deduplicate, and review biased columns before fine-tuning"
        } else {
            "ok — data is safe to fine-tune on as-is"
        },
    }
}

// ── PII ──────────────────────────────────────────────────────────────────────

fn scan_pii(df: &DataFrame, col_names: &[String]) -> PiiSection {
    let mut flagged_columns = Vec::new();
    let mut total = 0usize;

    for name in col_names {
        let Ok(column) = df.column(name.as_str()) else {
            continue;
        };
        let Ok(strings) = column.str() else {
            // PII literals are string-shaped (RRN, phone, email, card, tokens).
            continue;
        };

        let mut findings = 0usize;
        let mut kinds = std::collections::BTreeSet::new();
        let mut masked_samples: Vec<String> = Vec::new();

        for value in strings.iter().flatten() {
            for f in scan_output_text(value) {
                findings += 1;
                kinds.insert(kind_label(f.kind).to_string());
                if masked_samples.len() < MAX_MASKED_SAMPLES {
                    masked_samples.push(f.redacted);
                }
            }
        }

        if findings > 0 {
            flagged_columns.push(PiiColumn {
                column: name.clone(),
                findings,
                kinds: kinds.into_iter().collect(),
                masked_samples,
            });
            total += findings;
        }
    }

    PiiSection {
        flagged_columns,
        total_findings: total,
    }
}

fn kind_label(kind: SecretKind) -> &'static str {
    match kind {
        SecretKind::ResidentRegistrationNumber => "resident_registration_number",
        SecretKind::PhoneNumber => "phone_number",
        SecretKind::Email => "email",
        SecretKind::CreditCard => "credit_card",
        SecretKind::ApiKey => "api_key",
        SecretKind::PrivateKey => "private_key",
        SecretKind::GenericSecret => "generic_secret",
    }
}

// ── Duplicates ───────────────────────────────────────────────────────────────

fn scan_duplicates(df: &DataFrame, col_names: &[String]) -> DuplicateSection {
    let rows = df.height().max(1);

    // Exact duplicate rows — is_duplicated() marks every row that repeats a row.
    let exact_duplicates = df
        .is_duplicated()
        .map(|b: BooleanChunked| b.iter().filter(|v| v == &Some(true)).count())
        .unwrap_or(0);
    let exact_row_rate = exact_duplicates as f64 / rows as f64;

    // Near-duplicate rate per text column (normalized whitespace).
    let mut near_duplicate_columns = Vec::new();
    for name in col_names {
        let Ok(column) = df.column(name.as_str()) else {
            continue;
        };
        let Ok(strings) = column.str() else {
            continue;
        };
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut dupes = 0usize;
        let mut non_null = 0usize;
        for value in strings.iter().flatten() {
            non_null += 1;
            let norm = normalize_ws(value);
            let entry = seen.entry(norm).or_insert(0);
            *entry += 1;
            if *entry > 1 {
                dupes += 1;
            }
        }
        if non_null > 0 {
            let rate = dupes as f64 / non_null as f64;
            if rate >= NEAR_DUP_THRESHOLD {
                near_duplicate_columns.push(NearDuplicateColumn {
                    column: name.clone(),
                    near_duplicate_rate: rate,
                });
            }
        }
    }

    DuplicateSection {
        exact_row_rate,
        near_duplicate_columns,
    }
}

/// Collapses whitespace runs and lowercases — a cheap near-duplicate signal.
/// Leading and trailing whitespace are dropped so that
/// `"  Summarize   the   report  "` and `"summarize the report"` match.
fn normalize_ws(text: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.extend(c.to_lowercase());
            prev_space = false;
        }
    }
    // Drop a trailing space left by a run of whitespace at the end.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

// ── Bias ─────────────────────────────────────────────────────────────────────

fn scan_bias(df: &DataFrame, col_names: &[String]) -> BiasSection {
    let mut flagged_columns = Vec::new();

    for name in col_names {
        let Ok(column) = df.column(name.as_str()) else {
            continue;
        };
        // Count each distinct value by its display form.
        let mut counts: HashMap<String, u64> = HashMap::new();
        for opt in column.as_materialized_series().iter() {
            if let polars::prelude::AnyValue::Null = opt {
                continue;
            }
            let key = display_value(&opt);
            *counts.entry(key).or_insert(0) += 1;
        }
        if counts.len() < 2 {
            continue; // a constant or single-value column is not a bias signal here
        }
        let cardinality = counts.len();
        if cardinality > MAX_CATEGORICAL_CARDINALITY {
            continue; // high-cardinality (id-like) columns are not categorical
        }

        let max_count = counts.values().copied().max().unwrap_or(1);
        let min_count = counts.values().copied().min().unwrap_or(1);
        let imbalance_ratio = max_count as f64 / min_count as f64;
        let total: u64 = counts.values().sum();
        let dominant_share = max_count as f64 / total as f64;

        if imbalance_ratio >= BIAS_IMBALANCE_THRESHOLD && dominant_share >= BIAS_DOMINANT_SHARE {
            let mut top: Vec<(String, u64)> = counts.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            top.truncate(MAX_TOP_CATEGORIES);
            flagged_columns.push(BiasColumn {
                column: name.clone(),
                imbalance_ratio,
                cardinality,
                dominant_share,
                top_categories: top,
            });
        }
    }

    BiasSection { flagged_columns }
}

fn display_value(v: &polars::prelude::AnyValue) -> String {
    match v {
        polars::prelude::AnyValue::String(s) => s.to_string(),
        other => other.to_string(),
    }
}

/// Renders a human-readable sanitization report.
pub fn render_report(report: &SanitizeReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Sanitization report — {} ({} rows × {} cols)\n",
        report.source, report.rows, report.columns
    ));
    out.push_str(&format!("  recommendation: {}\n", report.recommendation));

    out.push_str("  PII:\n");
    if report.pii.flagged_columns.is_empty() {
        out.push_str("    none detected\n");
    } else {
        for c in &report.pii.flagged_columns {
            out.push_str(&format!(
                "    {} — {} finding(s) [{}], masked samples: {}\n",
                c.column,
                c.findings,
                c.kinds.join(", "),
                c.masked_samples.join(", ")
            ));
        }
    }

    out.push_str("  Duplicates:\n");
    out.push_str(&format!(
        "    exact row rate: {:.2}%\n",
        report.duplicates.exact_row_rate * 100.0
    ));
    for c in &report.duplicates.near_duplicate_columns {
        out.push_str(&format!(
            "    near-dup column {} — {:.2}%\n",
            c.column,
            c.near_duplicate_rate * 100.0
        ));
    }

    out.push_str("  Bias:\n");
    if report.bias.flagged_columns.is_empty() {
        out.push_str("    none flagged\n");
    } else {
        for c in &report.bias.flagged_columns {
            out.push_str(&format!(
                "    {} — imbalance {:.1}x (cardinality {}), dominant share {:.0}%\n",
                c.column,
                c.imbalance_ratio,
                c.cardinality,
                c.dominant_share * 100.0
            ));
            for (cat, cnt) in &c.top_categories {
                out.push_str(&format!("      {}: {}\n", cat, cnt));
            }
        }
    }
    out
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::Column;

    fn df_from_cols(cols: &[(&str, Vec<Option<&str>>)]) -> DataFrame {
        let mut out = DataFrame::empty();
        for (name, vals) in cols {
            let s: Column = Column::new(name.to_string().into(), vals.to_vec());
            out.with_column(s).expect("column add");
        }
        out
    }

    #[test]
    fn flags_pii_in_text_column() {
        let df = df_from_cols(&[
            (
                "name",
                vec![Some("홍길동"), Some("010-1234-5678"), Some("김철수")],
            ),
            (
                "text",
                vec![
                    Some("contact: sk-ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
                    Some("ok"),
                    Some("ok"),
                ],
            ),
        ]);
        let r = sanitize_df(&df, "test.csv");
        assert!(
            r.pii
                .flagged_columns
                .iter()
                .any(|c| c.kinds.iter().any(|k| k == "api_key")),
            "{:?}",
            r.pii
        );
        assert!(
            r.pii
                .flagged_columns
                .iter()
                .any(|c| c.kinds.iter().any(|k| k == "phone_number")),
            "{:?}",
            r.pii
        );
        // Raw values never appear in masked samples.
        for c in &r.pii.flagged_columns {
            for s in &c.masked_samples {
                assert!(!s.contains("1234567"), "원본 노출: {}", s);
            }
        }
    }

    #[test]
    fn clean_data_passes() {
        let df = df_from_cols(&[
            ("title", vec![Some("Q3 report"), Some("Q2 report")]),
            ("region", vec![Some("seoul"), Some("busan")]),
        ]);
        let r = sanitize_df(&df, "clean.csv");
        assert_eq!(r.pii.total_findings, 0);
        assert_eq!(r.duplicates.exact_row_rate, 0.0);
        assert!(r.bias.flagged_columns.is_empty());
        assert_eq!(r.recommendation, "ok — data is safe to fine-tune on as-is");
    }

    #[test]
    fn detects_exact_duplicate_rows() {
        let df = df_from_cols(&[
            ("a", vec![Some("x"), Some("x"), Some("y")]),
            ("b", vec![Some("1"), Some("1"), Some("2")]),
        ]);
        let r = sanitize_df(&df, "dup.csv");
        assert!(
            (r.duplicates.exact_row_rate - 2.0 / 3.0).abs() < 1e-9,
            "rate={}",
            r.duplicates.exact_row_rate
        );
    }

    #[test]
    fn detects_near_duplicate_text_column() {
        let df = df_from_cols(&[
            ("a", vec![Some("x"), Some("x"), Some("x"), Some("other")]),
            (
                "note",
                vec![
                    Some("  Summarize   the   report  "),
                    Some("summarize the report"),
                    Some("summarize the report"),
                    Some("other"),
                ],
            ),
        ]);
        let r = sanitize_df(&df, "near.csv");
        let note = r
            .duplicates
            .near_duplicate_columns
            .iter()
            .find(|c| c.column == "note");
        assert!(note.is_some(), "{:?}", r.duplicates.near_duplicate_columns);
        assert!(
            (note.unwrap().near_duplicate_rate - 0.5).abs() < 1e-9,
            "{:?}",
            r.duplicates.near_duplicate_columns
        );
    }

    #[test]
    fn flags_imbalanced_categorical_column() {
        let df = df_from_cols(&[(
            "label",
            vec![
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("positive"),
                Some("negative"),
            ],
        )]);
        let r = sanitize_df(&df, "bias.csv");
        let flag = r.bias.flagged_columns.iter().find(|c| c.column == "label");
        assert!(flag.is_some(), "{:?}", r.bias.flagged_columns);
        assert!(flag.unwrap().imbalance_ratio >= 10.0);
        assert!(flag.unwrap().dominant_share >= 0.5);
    }

    #[test]
    fn balanced_categorical_column_is_not_flagged() {
        let df = df_from_cols(&[(
            "label",
            vec![
                Some("positive"),
                Some("negative"),
                Some("positive"),
                Some("negative"),
            ],
        )]);
        let r = sanitize_df(&df, "balanced.csv");
        assert!(
            r.bias.flagged_columns.is_empty(),
            "{:?}",
            r.bias.flagged_columns
        );
    }
}
