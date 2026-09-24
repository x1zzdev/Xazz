use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn cli_check_json_reports_semantic_source_location() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let source =
        std::env::temp_dir().join(format!("xazz-cli-span-{}-{nonce}.xzz", std::process::id()));
    std::fs::write(
        &source,
        "type Air = { temperature_c: float }\nv raw = load(\"air.csv\") :: Air\nv warm = raw |> filter(temperture_c > 20)\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xazz"))
        .args(["check", source.to_str().unwrap(), "--json"])
        .env("XAZZ_LANG", "en")
        .output()
        .unwrap();
    std::fs::remove_file(source).unwrap();

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error_count"], 1);
    assert_eq!(report["errors"][0]["line"], 3);
    assert_eq!(report["errors"][0]["col"], 24);
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("temperture_c")
    );
}
