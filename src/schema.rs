use anyhow::{Context, Result};
use std::fs;
use std::io::Read;

// ─── 상수 ──────────────────────────────────────────────────────────────────

/// 스키마 추론에 검사할 최대 샘플 행 수.
const SCHEMA_SAMPLE_ROWS: usize = 100;

// ─── 타입 추론 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ColType {
    Bool,
    Int,
    Float,
    String,
}

/// 단일 셀 값에서 타입을 추론합니다.
fn infer_type(value: &str) -> ColType {
    let trimmed = value.trim();
    match trimmed.to_lowercase().as_str() {
        "true" | "false" => return ColType::Bool,
        _ => {}
    }
    if trimmed.parse::<i64>().is_ok() {
        return ColType::Int;
    }
    if trimmed.parse::<f64>().is_ok() {
        return ColType::Float;
    }
    ColType::String
}

/// 두 타입을 병합합니다 (타입 승격 규칙).
///
/// Bool + Bool  = Bool
/// Int  + Int   = Int
/// Float+ Float = Float
/// Str  + Str   = String
/// Int  + Float = Float
/// Bool + Int   = String
/// Bool + Float = String
/// Anything + String = String
fn merge_type(a: ColType, b: ColType) -> ColType {
    use ColType::*;
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

// ─── 이름 생성 헬퍼 ─────────────────────────────────────────────────────────

/// 파일 경로에서 PascalCase 타입 이름을 생성합니다.
///
/// 예)
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

/// 파일 경로에서 변수 이름을 생성합니다.
///
/// 예)
/// - `seoul_air.csv`   → `air`
/// - `weather_data.csv` → `weather`
/// - `population.csv`   → `population`
///
/// 규칙: 마지막 언더스코어 이후 세그먼트 사용, 없으면 전체 stem 사용.
fn filename_to_var_name(path: &str) -> std::string::String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data");

    // 언더스코어가 있을 때 마지막 세그먼트, 없으면 전체
    let segments: Vec<&str> = stem.split('_').collect();
    if segments.len() >= 2 {
        // 마지막 세그먼트가 너무 짧거나 숫자면 두 번째 마지막 사용
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

// ─── 스키마 추론 ────────────────────────────────────────────────────────────

/// CSV 파일을 읽어 xazz 타입 정의 + load 문을 생성합니다.
///
/// 최대 100행 샘플만 검사합니다.
pub fn infer_csv_schema(csv_path: &str) -> Result<std::string::String> {
    // 1) 파일을 바이트로 읽음
    let mut file = fs::File::open(csv_path)
        .with_context(|| format!("failed to open CSV file '{}'.", csv_path))?;
    let mut raw_bytes = Vec::new();
    file.read_to_end(&mut raw_bytes)
        .with_context(|| format!("failed to read CSV file '{}'", csv_path))?;

    // 2) EUC-KR(CP949) 감지 및 UTF-8 디코딩
    let content = if raw_bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        // BOM이 있으면 UTF-8 BOM 제거 후 사용
        std::string::String::from_utf8(raw_bytes[3..].to_vec())
            .map_err(|e| anyhow::anyhow!("UTF-8 디코딩 실패: {}", e))?
    } else {
        // EUC-KR로 디코딩 시도, 실패하면 UTF-8로 fallback
        let (cow, _, had_errors) = encoding_rs::EUC_KR.decode(&raw_bytes);
        if had_errors {
            // EUC-KR 실패 시 UTF-8 fallback
            std::string::String::from_utf8(raw_bytes)
                .map_err(|e| anyhow::anyhow!("UTF-8 디코딩도 실패: {}", e))?
        } else {
            cow.into_owned()
        }
    };

    // 3) 메모리에서 CSV Reader 생성
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

    // 열별 현재 추론 타입 (초기값 없음 → Option)
    let mut col_types: Vec<Option<ColType>> = vec![None; col_count];
    // 열별 nullable 여부
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

    // 타입이 한 번도 채워지지 않은 열(모두 공백)은 String 처리
    let col_types: Vec<ColType> = col_types
        .into_iter()
        .map(|t| t.unwrap_or(ColType::String))
        .collect();

    // ─── 코드 생성 ───────────────────────────────────────────────────────────
    let type_name = filename_to_type_name(csv_path);
    let var_name = filename_to_var_name(csv_path);

    let mut output = std::string::String::new();
    output.push_str(&format!("type {} = {{\n", type_name));

    for (i, header) in headers.iter().enumerate() {
        let base = match &col_types[i] {
            ColType::Bool => "bool",
            ColType::Int => "int",
            ColType::Float => "float",
            ColType::String => "string",
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

// ─── Import 명령어 ──────────────────────────────────────────────────────────

/// CSV 파일 경로에서 xazz.toml이 있는 프로젝트 루트 디렉토리를 탐색합니다.
///
/// 예) `a/data/seoul.csv` → `a/` (a/xazz.toml 이 존재하므로)
fn find_project_root(csv_path: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(csv_path);
    // CSV 파일의 부모 디렉토리부터 시작해서 상위로 올라가며 xazz.toml 탐색
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

/// CSV 파일을 읽어 스키마를 추론하고 main.xzz에 추가합니다.
pub fn import_csv(file: &str) -> Result<()> {
    let generated = infer_csv_schema(file)?;

    // CSV 경로에서 프로젝트 루트(xazz.toml이 있는 디렉토리)를 찾고,
    // 없으면 현재 디렉토리를 폴백으로 사용합니다.
    let main_xzz_path = match find_project_root(file) {
        Some(root) => root.join("main.xzz"),
        None => std::path::PathBuf::from("main.xzz"),
    };

    // main.xzz 읽기 (없으면 빈 파일로 처리)
    let current = if main_xzz_path.exists() {
        fs::read_to_string(&main_xzz_path)
            .with_context(|| format!("failed to read {}", main_xzz_path.display()))?
    } else {
        std::string::String::new()
    };

    // 이미 같은 타입 정의가 존재하면 스킵
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

    // 기존 내용 끝 + 빈 줄 + 생성 코드 + 마지막 줄바꿈
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
