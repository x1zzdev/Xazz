// xazz-exec/src/schema_infer.rs — schema inference for `xazz import` (issue #55)
//
// The CLI binary (xazz) must stay Polars-free (architecture rule), so columnar
// schema inference for .parquet/.arrow lives here, in the engine crate, and is
// invoked by the CLI via `xazz-exec --schema <file>`.
//
// Output matches the CSV path in src/schema.rs (Rust CLI):
//   type Name = {
//       col: int,
//       opt_col: Option<float>
//   };
//
//   v name = load("path") :: Name

use std::path::Path;

use polars::io::ipc::IpcScanOptions;
use polars::lazy::dsl::UnifiedScanArgs;
use polars::prelude::{DataType, IdxSize, LazyFrame, PlRefPath, ScanArgsParquet};

/// Number of leading rows inspected to determine nullability per column.
const SAMPLE_ROWS: usize = 100;

/// Produce a `type` block + `load` statement for a columnar source
/// (.parquet / .arrow / .ipc / .feather), mirroring the CSV importer's output.
pub fn infer_columnar_schema(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    // Build a lazy scan restricted to SAMPLE_ROWS so only the head is analyzed.
    let lf: LazyFrame = match ext.as_str() {
        "parquet" | "pq" => LazyFrame::scan_parquet(
            PlRefPath::new(path),
            ScanArgsParquet {
                n_rows: Some(SAMPLE_ROWS),
                ..Default::default()
            },
        )?,
        "arrow" | "ipc" | "feather" => LazyFrame::scan_ipc(
            PlRefPath::new(path),
            IpcScanOptions::default(),
            UnifiedScanArgs::default(),
        )?,
        other => {
            return Err(format!(
                "unsupported columnar format '{}' for schema inference — use .parquet or .arrow",
                other
            )
            .into());
        }
    };

    // Column names + dtypes come from the scan schema (metadata).
    let schema = lf.clone().collect_schema()?;
    let names: Vec<String> = schema.iter_names().map(|n| n.to_string()).collect();
    let dtypes: Vec<DataType> = schema.iter_values().cloned().collect();

    // Nullability needs actual data — collect the (already limited) sample.
    let sample = lf.limit(SAMPLE_ROWS as IdxSize).collect()?;
    let nullable: Vec<bool> = names
        .iter()
        .map(|name| {
            sample
                .column(name)
                .map(|s| s.null_count() > 0)
                .unwrap_or(false)
        })
        .collect();

    // ── code generation ───────────────────────────────────────────────────
    let type_name = type_name_from_path(path);
    let var_name = var_name_from_path(path);

    let mut out = String::new();
    out.push_str(&format!("type {} = {{\n", type_name));
    for (i, name) in names.iter().enumerate() {
        let base = match &dtypes[i] {
            DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64 => "int",
            DataType::Float32 | DataType::Float64 => "float",
            DataType::Boolean => "bool",
            _ => "string",
        };
        let t = if nullable[i] {
            format!("Option<{}>", base)
        } else {
            base.to_owned()
        };
        let comma = if i + 1 < names.len() { "," } else { "" };
        out.push_str(&format!("    {}: {}{}\n", name, t, comma));
    }
    out.push_str("};\n\n");
    out.push_str(&format!(
        "v {} = load(\"{}\") :: {}",
        var_name, path, type_name
    ));

    Ok(out)
}

/// Convert a file path stem to a PascalCase type name.
/// (mirrors src/schema.rs in the CLI)
fn type_name_from_path(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");
    stem.split(|c: char| c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Convert a file path stem to a variable name (last underscore segment).
/// (mirrors src/schema.rs in the CLI)
fn var_name_from_path(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data");
    let segments: Vec<&str> = stem.split('_').collect();
    if segments.len() >= 2 {
        let last = *segments.last().unwrap_or(&stem);
        if last.len() >= 2 && last.parse::<u64>().is_err() {
            last.to_lowercase()
        } else {
            segments[segments.len() - 2].to_lowercase()
        }
    } else {
        stem.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_names_pascal_case() {
        assert_eq!(type_name_from_path("data/seoul_air.csv"), "SeoulAir");
        assert_eq!(type_name_from_path("weather_data.csv"), "WeatherData");
        assert_eq!(type_name_from_path("population.csv"), "Population");
    }

    #[test]
    fn var_names_use_last_segment() {
        assert_eq!(var_name_from_path("data/seoul_air.csv"), "air");
        assert_eq!(var_name_from_path("weather_data.csv"), "data");
        assert_eq!(var_name_from_path("population.csv"), "population");
        // numeric last segment → second-to-last
        assert_eq!(var_name_from_path("data_2026.parquet"), "data");
    }
}
