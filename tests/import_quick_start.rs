use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn quick_start_import_generates_checkable_source() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("xazz-quick-start-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_xazz");
    assert_success(
        Command::new(cli)
            .current_dir(&root)
            .args(["new", "demo"])
            .output()
            .unwrap(),
    );
    let project = root.join("demo");
    assert_success(
        Command::new(cli)
            .current_dir(&project)
            .args(["import", "data/sample.csv"])
            .output()
            .unwrap(),
    );
    let checked = Command::new(cli)
        .current_dir(&project)
        .args(["check", "main.xzz", "--json"])
        .output()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert_success(checked);
}
