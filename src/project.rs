use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// 프로젝트명이 단일 안전한 경로 세그먼트인지 검증한다.
/// 경로 구분자, `..`, 절대 경로, 드라이브 접두사를 거부한다.
fn validate_project_name(name: &str) -> Result<()> {
    let invalid_hint = |c: &str| {
        format!(
            "프로젝트 생성 실패: '{}' 은(는) 유효하지 않은 프로젝트명입니다.\n\
             프로젝트명에는 '{}' 을(를) 포함할 수 없습니다. 영숫자, '-', '_' 만 사용하세요.",
            name, c
        )
    };

    if name.is_empty() {
        bail!("프로젝트 생성 실패: 프로젝트명이 비어 있습니다.");
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
            "프로젝트 생성 실패: '{}' 디렉터리가 이미 존재합니다.\n\
             다른 이름을 사용하거나 기존 디렉터리를 삭제하세요.",
            name
        );
    }

    // 루트 + data/ 디렉터리 생성
    fs::create_dir_all(root.join("data"))
        .with_context(|| format!("'{}' 디렉터리 생성에 실패했습니다.", name))?;

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
        .with_context(|| "data/sample.csv 파일 작성에 실패했습니다.".to_string())?;

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
        .with_context(|| "example.xzz 파일 작성에 실패했습니다.".to_string())?;

    // main.xzz — 빈 스타터 파일
    let main_xzz = "// Xazz Project\n// Edit this file or run: xazz run example.xzz\n\n";
    fs::write(root.join("main.xzz"), main_xzz)
        .with_context(|| "main.xzz 파일 작성에 실패했습니다.".to_string())?;

    // xazz.toml 작성 — name 값은 TOML 문자열로 이스케이프
    let toml_name = name.replace('\\', "\\\\").replace('"', "\\\"");
    let toml_content = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", toml_name);
    fs::write(root.join("xazz.toml"), toml_content)
        .with_context(|| "xazz.toml 파일 작성에 실패했습니다.".to_string())?;

    println!("✅  프로젝트 '{}' 생성 완료!", name);
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
