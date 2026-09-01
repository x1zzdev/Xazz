// src/sde.rs — Synthetic Data Engine (CLI) v0.1
//
// `xazz sde --rows N --output path.jsonl` generates real synthetic data and
// writes it to a JSONL file. Previously it was only a Preview stub placeholder.
//
// Target: synthetic training data pairs — randomly generated records of the
// (feature column, label column) form for regression/classification pipelines.
//
// Random: self-contained SplitMix64 implementation with no external crate (same as xazz-exec dp.rs).
//   The seed is fixed (deterministic), so repeated runs reproduce the same data.

use std::io::Write;
use std::path::Path;

/// Simple SplitMix64 PRNG — generates a deterministic sequence from a fixed seed.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform distribution on [0,1)
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// A single record — a synthetic training pair.
///
/// - `feature_x`, `feature_y`: input features (normal-distribution-ish, [-10, 10])
/// - `category`: one of 4 categories (for classification)
/// - `label`: linear relationship + noise (for regression)
#[derive(serde::Serialize, serde::Deserialize)]
struct Pair {
    feature_x: f64,
    feature_y: f64,
    category: String,
    label: f64,
}

/// Generate `rows` synthetic data pairs and write them to the `output` path (JSONL).
///
/// Overwrites an existing file. Creates the parent directory if missing.
pub fn generate(rows: usize, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if rows == 0 {
        return Err("rows must be greater than 0.".into());
    }

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut rng = SplitMix64::new(0x5EED_5EED_5EED_5EED);
    let categories = ["alpha", "beta", "gamma", "delta"];

    let mut file = std::fs::File::create(output)?;
    for _ in 0..rows {
        // Normal distribution approximation: sum of 6 uniform variables via the central limit theorem → N(0,1)
        let gauss = |rng: &mut SplitMix64| {
            let mut s = 0.0;
            for _ in 0..6 {
                s += rng.next_f64();
            }
            (s - 3.0) * 2.0
        };

        let feature_x = gauss(&mut rng);
        let feature_y = gauss(&mut rng);
        let category = categories[(rng.next_u64() % categories.len() as u64) as usize];
        // label = 2·x - 1·y + category_offset + noise
        let cat_offset = match category {
            "alpha" => 0.0,
            "beta" => 2.0,
            "gamma" => -1.0,
            _ => 1.0,
        };
        let label = 2.0 * feature_x - 1.0 * feature_y + cat_offset + gauss(&mut rng) * 0.5;

        let record = Pair {
            feature_x,
            feature_y,
            category: category.to_string(),
            label,
        };
        let line = serde_json::to_string(&record)?;
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_requested_row_count() {
        let dir = std::env::temp_dir().join(format!("xazz_sde_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pairs.jsonl");

        generate(500, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 500, "행 수 불일치");
        // Each line must be valid JSON
        for line in text.lines() {
            serde_json::from_str::<Pair>(line).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deterministic_with_fixed_seed() {
        let dir = std::env::temp_dir().join(format!("xazz_sde_det_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        generate(200, &a).unwrap();
        generate(200, &b).unwrap();
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap(),
            "동일 시드는 동일 데이터를 생성해야 한다"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_zero_rows() {
        let path = std::path::Path::new("/tmp/xazz_sde_zero.jsonl");
        assert!(generate(0, path).is_err());
    }
}
