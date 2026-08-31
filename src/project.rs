use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// 프로젝트명이 단일 안전한 경로 세그먼트인지 검증한다.
/// 경로 구분자, `..`, 절대 경로, 드라이브 접두사를 거부한다.
fn validate_project_name(name: &str) -> Result<()> {
    let invalid_hint = |c: &str| {
        format!(
            "project creation failed: '{}' is not a valid project name.\n\
             Project names cannot contain '{}'. Use only alphanumerics, '-', and '_'.",
            name, c
        )
    };

    if name.is_empty() {
        bail!("project creation failed: project name is empty.");
    }
    if name == "." || name == ".." {
        bail!(invalid_hint(name));
    }
    if name.starts_with('/') || name.starts_with('\\') || name.contains("..") {
        bail!(invalid_hint("경로 구분자 / .."));
    }
    // Windows 드라이브 접두사 (C:\) 및 URL 스킴 거부
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        bail!(invalid_hint("드라이브 문자 (:)"));
    }
    if name.contains('/') || name.contains('\\') {
        bail!(invalid_hint("경로 구분자"));
    }
    Ok(())
}

/// 새 Xazz 프로젝트 디렉터리를 생성합니다.
///
/// 생성 구조:
/// ```text
/// {name}/
/// ├── data/
/// │   └── sample.csv
/// ├── example.xzz
/// ├── main.xzz
/// └── xazz.toml
/// ```
pub fn create_project(name: &str) -> Result<()> {
    // ── 프로젝트명 검증: 단일 안전 경로 세그먼트만 허용 (디렉터리 트래버설 방지)
    validate_project_name(name)?;

    let root = Path::new(name);

    // 이미 존재하면 실패
    if root.exists() {
        bail!(
            "project creation failed: directory '{}' already exists.\n\
             choose a different name or delete the existing directory.",
            name
        );
    }

    // 루트 + data/ 디렉터리 생성
    fs::create_dir_all(root.join("data"))
        .with_context(|| format!("failed to create directory '{}'.", name))?;

    // data/sample.csv — 즉시 실행 가능한 샘플 데이터
    let sample_csv = "\
station,pm10,pm25,date
Gangnam,45.2,23.1,2026-01-01
Gangseo,52.3,28.4,2026-01-02
Jongno,38.1,19.5,2026-01-03
Mapo,61.4,33.2,2026-01-04
Seocho,42.8,21.7,2026-01-05
Nowon,33.7,16.8,2026-01-06
Dobong,55.9,29.1,2026-01-07
Seodaemun,47.3,24.6,2026-01-08
Yongsan,39.2,20.3,2026-01-09
Songpa,68.1,36.4,2026-01-10
";
    fs::write(root.join("data").join("sample.csv"), sample_csv)
        .with_context(|| "failed to write data/sample.csv.".to_string())?;

    // example.xzz — 즉시 실행 가능한 파이프라인 예제
    let example_xzz = r#"// Xazz Quick Start Example
// Run: xazz run example.xzz
// Export: xazz run example.xzz --output result.csv

type AirQuality = {
    station: string,
    pm10: float,
    pm25: float,
    date: string,
}

v data = load("data/sample.csv") :: AirQuality

v result = data
    |> filter(col("pm10") > 40.0)
    |> orderBy("pm10", desc: true)
"#;
    fs::write(root.join("example.xzz"), example_xzz)
        .with_context(|| "failed to write example.xzz.".to_string())?;

    // main.xzz — 빈 스타터 파일
    let main_xzz = "// Xazz Project\n// Edit this file or run: xazz run example.xzz\n\n";
    fs::write(root.join("main.xzz"), main_xzz)
        .with_context(|| "failed to write main.xzz.".to_string())?;

    // xazz.toml 작성 — name 값은 TOML 문자열로 이스케이프
    let toml_name = name.replace('\\', "\\\\").replace('"', "\\\"");
    let toml_content = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", toml_name);
    fs::write(root.join("xazz.toml"), toml_content)
        .with_context(|| "failed to write xazz.toml.".to_string())?;

    println!("✅  project '{}' created!", name);
    println!();
    println!("   {}/", name);
    println!("   ├── data/");
    println!("   │   └── sample.csv");
    println!("   ├── example.xzz");
    println!("   ├── main.xzz");
    println!("   └── xazz.toml");
    println!();
    println!("   Quick Start:");
    println!("   $ cd {}", name);
    println!("   $ xazz run example.xzz");
    println!("   $ xazz run example.xzz --output result.csv");

    Ok(())
}
