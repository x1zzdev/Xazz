use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// Verify that the project name is a single safe path segment.
/// Reject path separators, `..`, absolute paths, and drive prefixes.
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
    // Reject Windows drive prefixes (C:\) and URL schemes
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        bail!(invalid_hint("드라이브 문자 (:)"));
    }
    if name.contains('/') || name.contains('\\') {
        bail!(invalid_hint("경로 구분자"));
    }
    Ok(())
}

/// Create a new Xazz project directory.
///
/// Generated structure:
/// ```text
/// {name}/
/// ├── data/
/// │   └── sample.csv
/// ├── example.xzz
/// ├── main.xzz
/// └── xazz.toml
/// ```
pub fn create_project(name: &str) -> Result<()> {
    // ── validate project name: allow only a single safe path segment (prevents directory traversal)
    validate_project_name(name)?;

    let root = Path::new(name);

    // Fail if it already exists
    if root.exists() {
        bail!(
            "project creation failed: directory '{}' already exists.\n\
             choose a different name or delete the existing directory.",
            name
        );
    }

    // Create the root + data/ directories
    fs::create_dir_all(root.join("data"))
        .with_context(|| format!("failed to create directory '{}'.", name))?;

    // data/sample.csv — ready-to-run sample data
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

    // example.xzz — ready-to-run pipeline example
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

    // main.xzz — empty starter file
    let main_xzz = "// Xazz Project\n// Edit this file or run: xazz run example.xzz\n\n";
    fs::write(root.join("main.xzz"), main_xzz)
        .with_context(|| "failed to write main.xzz.".to_string())?;

    // Write xazz.toml — escape the name value as a TOML string
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
