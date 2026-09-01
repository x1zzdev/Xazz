use anyhow::{Context, Result};
use std::fs;
use std::io::Read;

// ─── constants ──────────────────────────────────────────────────────────────

/// Maximum number of sample rows to inspect for schema inference.
const SCHEMA_SAMPLE_ROWS: usize = 100;

// ─── type inference ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum InferredType {
    Bool,
    Int,
    Float,
    String,
}

/// Infer the type from a single cell value.
fn infer_type(value: &str) -> InferredType {
    let trimmed = value.trim();
    match trimmed.to_lowercase().as_str() {
        "true" | "false" => return InferredType::Bool,
        _ => {}
    }
    if trimmed.parse::<i64>().is_ok() {
        return InferredType::Int;
    }
    if trimmed.parse::<f64>().is_ok() {
        return InferredType::Float;
    }
    InferredType::String
}

/// Merge two types (type promotion rules).
///
/// Bool + Bool  = Bool
/// Int  + Int   = Int
/// Float+ Float = Float
/// Str  + Str   = String
/// Int  + Float = Float
/// Bool + Int   = String
/// Bool + Float = String
/// Anything + String = String
fn merge_type(a: InferredType, b: InferredType) -> InferredType {
    use InferredType::*;
    match (a, b) {
        (Bool, Bool) => Bool,
        (Int, Int) => Int,
        (Float, Float) => Float,
        (String, String) => String,
        (Int, Float) | (Float, Int) => Float,
        (Bool, Int) | (Int, Bool) => String,
        (Bool, Float) | (Float, Bool) => String,
        _ => String,
    }
}

// ─── name generation helpers ──────────────────────────────────────────────────

/// Generate a PascalCase type name from a file path.
///
/// Example)
/// - `data/seoul_air.csv` → `SeoulAir`
/// - `weather_data.csv`   → `WeatherData`
/// - `population.csv`     → `Population`
fn filename_to_type_name(path: &str) -> std::string::String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    stem.split(|c: char| c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                None => std::string::String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<std::string::String>() + chars.as_str()
                }
            }
        })
        .collect::<std::string::String>()
}

/// Generate a variable name from a file path.
///
/// Example)
/// - `seoul_air.csv`   → `air`
/// - `weather_data.csv` → `weather`
/// - `population.csv`   → `population`
///
/// Rule: use the segment after the last underscore, or the whole stem if none.
fn filename_to_var_name(path: &str) -> std::string::String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data");

    // When there is an underscore, use the last segment; otherwise the whole
    let segments: Vec<&str> = stem.split('_').collect();
    if segments.len() >= 2 {
        // If the last segment is too short or numeric, use the second-to-last
        let last = *segments.last().unwrap_or(&stem);
        if last.len() >= 2 && last.parse::<u64>().is_err() {
            last.to_lowercase()
        } else if segments.len() >= 2 {
            segments[segments.len() - 2].to_lowercase()
        } else {
            stem.to_lowercase()
        }
    } else {
        stem.to_lowercase()
    }
}

// ─── schema inference ──────────────────────────────────────────────────────────

/// Read a CSV file and generate a xazz type definition + load statement.
///
/// Only inspects a sample of up to 100 rows.
pub fn infer_csv_schema(csv_path: &str) -> Result<std::string::String> {
    // 1) Read the file as bytes
    let mut file = fs::File::open(csv_path)
        .with_context(|| format!("failed to open CSV file '{}'.", csv_path))?;
    let mut raw_bytes = Vec::new();
    file.read_to_end(&mut raw_bytes)
        .with_context(|| format!("failed to read CSV file '{}'", csv_path))?;

    // 2) Detect EUC-KR(CP949) and decode as UTF-8
    let content = if raw_bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        // If there is a BOM, strip the UTF-8 BOM and use it
        std::string::String::from_utf8(raw_bytes[3..].to_vec())
            .map_err(|e| anyhow::anyhow!("UTF-8 디코딩 실패: {}", e))?
    } else {
        // Try decoding as EUC-KR, fall back to UTF-8 on failure
        let (cow, _, had_errors) = encoding_rs::EUC_KR.decode(&raw_bytes);
        if had_errors {
            // Fall back to UTF-8 when EUC-KR fails
            std::string::String::from_utf8(raw_bytes)
                .map_err(|e| anyhow::anyhow!("UTF-8 디코딩도 실패: {}", e))?
        } else {
            cow.into_owned()
        }
    };

    // 3) Create the CSV reader in memory
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(content.as_bytes());

    let headers: Vec<std::string::String> = rdr
        .headers()
        .with_context(|| "CSV 헤더를 읽는 데 실패했습니다.")?
        .iter()
        .map(|h| h.to_owned())
        .collect();

    let col_count = headers.len();

    // Current inferred type per column (no initial value → Option)
    let mut col_types: Vec<Option<InferredType>> = vec![None; col_count];
    // Whether each column is nullable
    let mut col_nullable: Vec<bool> = vec![false; col_count];

    for result in rdr.records().take(SCHEMA_SAMPLE_ROWS) {
        let record = result.with_context(|| "CSV 레코드 읽기 실패")?;

        for (i, field) in record.iter().enumerate() {
            if i >= col_count {
                break;
            }
            let trimmed = field.trim();
            if trimmed.is_empty() {
                col_nullable[i] = true;
                continue;
            }
            let inferred = infer_type(trimmed);
            col_types[i] = Some(match col_types[i].take() {
                None => inferred,
                Some(existing) => merge_type(existing, inferred),
            });
        }
    }

    // Columns never populated with a type (all blank) are treated as String
    let col_types: Vec<InferredType> = col_types
        .into_iter()
        .map(|t| t.unwrap_or(InferredType::String))
        .collect();

    // ─── code generation ───────────────────────────────────────────────────────────
    let type_name = filename_to_type_name(csv_path);
    let var_name = filename_to_var_name(csv_path);

    let mut output = std::string::String::new();
    output.push_str(&format!("type {} = {{\n", type_name));

    for (i, header) in headers.iter().enumerate() {
        let base = match &col_types[i] {
            InferredType::Bool => "bool",
            InferredType::Int => "int",
            InferredType::Float => "float",
            InferredType::String => "string",
        };
        let type_str = if col_nullable[i] {
            format!("Option<{}>", base)
        } else {
            base.to_owned()
        };
        let comma = if i + 1 < col_count { "," } else { "" };
        output.push_str(&format!("    {}: {}{}\n", header, type_str, comma));
    }

    output.push_str("};\n");
    output.push('\n');
    output.push_str(&format!(
        "v {} = load(\"{}\") :: {}",
        var_name, csv_path, type_name
    ));

    Ok(output)
}

// ─── import command ───────────────────────────────────────────────────────────

/// Search for the project root directory (containing xazz.toml) from a CSV file path.
///
/// Example) `a/data/seoul.csv` → `a/` (because a/xazz.toml exists)
fn find_project_root(csv_path: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(csv_path);
    // Start from the CSV file's parent directory and walk upward looking for xazz.toml
    let mut dir = path.parent()?;
    loop {
        if dir.join("xazz.toml").exists() {
            return Some(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return None,
        }
    }
}

/// Read a CSV file, infer its schema, and append it to main.xzz.
pub fn import_csv(file: &str) -> Result<()> {
    let generated = infer_csv_schema(file)?;

    // Find the project root (directory containing xazz.toml) from the CSV path,
    // falling back to the current directory if not found.
    let main_xzz_path = match find_project_root(file) {
        Some(root) => root.join("main.xzz"),
        None => std::path::PathBuf::from("main.xzz"),
    };

    // Read main.xzz (treat as an empty file if not present)
    let current = if main_xzz_path.exists() {
        fs::read_to_string(&main_xzz_path)
            .with_context(|| format!("failed to read {}", main_xzz_path.display()))?
    } else {
        std::string::String::new()
    };

    // Skip if the same type definition already exists
    let type_name = filename_to_type_name(file);
    let type_marker = format!("type {} =", type_name);
    if current.contains(&type_marker) {
        println!(
            "⚠️  '{}' 타입은 이미 {} 에 정의되어 있습니다. 스킵합니다.",
            type_name,
            main_xzz_path.display()
        );
        return Ok(());
    }

    // existing content end + blank line + generated code + trailing newline
    let updated = format!("{}\n\n{}\n", current.trim_end(), generated);

    fs::write(&main_xzz_path, &updated)
        .with_context(|| format!("failed to write {}", main_xzz_path.display()))?;

    println!(
        "✅  '{}' 스키마 추론 완료 → {} 에 추가되었습니다.",
        file,
        main_xzz_path.display()
    );
    println!();
    println!("{}", generated);

    Ok(())
}
