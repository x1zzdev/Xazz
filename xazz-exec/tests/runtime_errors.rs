// 런타임 실패가 프로세스 실패로 전파되고 후속 실행·성공 출력을 중단하는지 검사한다.
// 각 실행은 독립 프로세스이므로 병렬 테스트에서 환경변수나 작업 경로를 공유하지 않는다.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(script: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "xazz_runtime_errors_{}_{}_{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("data.csv"), "a,b\n1,10\n2,20\n").unwrap();
        std::fs::write(
            dir.join("pipeline.xzz"),
            format!("type S = {{ a: int, b: int }};\n{script}"),
        )
        .unwrap();
        Self(dir)
    }

    fn run(&self, envs: &[(&str, &str)]) -> Output {
        self.run_with_output(envs, "final.csv")
    }

    fn run_with_output(&self, envs: &[(&str, &str)], output_path: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xazz-exec"))
            .args(["pipeline.xzz", "--output", output_path])
            .current_dir(&self.0)
            .env("XAZZ_LANG", "en")
            .env("XAZZ_COLLECT_DIAGNOSTICS", "1")
            .env_remove("XAZZ_STREAMING")
            .env_remove("POLARS_AUTO_NEW_STREAMING")
            .env_remove("POLARS_FORCE_NEW_STREAMING")
            .envs(envs.iter().copied())
            .output()
            .expect("실행 바이너리를 시작해야 함")
    }

    fn assert_aborted(&self, output: &Output, failed_pipeline: usize, reason: &str) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "실패 종료 코드가 필요함:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("[xazz RUNTIME ERROR] Pipeline #{failed_pipeline}"))
                && stderr.contains(reason),
            "원래 런타임 오류가 필요함:\n{stderr}"
        );
        assert!(
            !stderr.contains(&format!("Pipeline #{}", failed_pipeline + 1)),
            "오류 다음 파이프라인을 실행하면 안 됨:\n{stderr}"
        );
        assert!(!stdout.contains("[xazz:timing]"), "{stdout}");
        assert!(!stdout.contains("[xazz:result]"), "{stdout}");
        assert!(!self.0.join("after.csv").exists(), "후속 save 실행 금지");
        assert!(
            !self.0.join("final.csv").exists(),
            "실패한 실행의 최종 CSV 출력 금지"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn runtime_failure_stops_remaining_pipelines_without_publishing_prior_result() {
    for streaming in ["1", "0", "auto"] {
        let fixture = Fixture::new(
            "v first = load(\"data.csv\") :: S;
             v broken = load(\"missing.csv\") :: S;
             v later = load(\"data.csv\") :: S |> save(\"after.csv\");",
        );
        let output = fixture.run(&[("XAZZ_STREAMING", streaming)]);
        fixture.assert_aborted(&output, 2, "missing.csv");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            stdout.matches("[xazz:collect]").count(),
            1,
            "오류 전 첫 번째 수집만 성공해야 함:\n{stdout}"
        );
    }
}

#[test]
fn invalid_streaming_configuration_is_a_process_failure() {
    for (name, value) in [
        ("XAZZ_STREAMING", "invalid"),
        ("POLARS_AUTO_NEW_STREAMING", "1"),
        ("POLARS_FORCE_NEW_STREAMING", "1"),
    ] {
        let fixture = Fixture::new(
            "v first = load(\"data.csv\") :: S;
             v later = load(\"data.csv\") :: S |> save(\"after.csv\");",
        );
        let output = fixture.run(&[(name, value)]);
        fixture.assert_aborted(&output, 1, name);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("[xazz:collect]"), "{stdout}");
    }
}

#[test]
fn output_creation_failure_preserves_error_without_success_markers() {
    for output_path in ["missing-parent/final.csv", "occupied"] {
        let fixture = Fixture::new("v result = load(\"data.csv\") :: S;");
        std::fs::create_dir(fixture.0.join("occupied")).unwrap();
        let output = fixture.run_with_output(&[], output_path);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "출력 저장 실패는 실패 종료해야 함:\n{stderr}"
        );
        assert!(stderr.contains("failed to create CSV file"), "{stderr}");
        assert!(
            stderr.contains(output_path),
            "대상 경로가 보존되어야 함:\n{stderr}"
        );
        assert!(
            stderr.contains("os error"),
            "원래 OS 오류가 보존되어야 함:\n{stderr}"
        );
        assert_eq!(
            stdout.matches("[xazz:collect]").count(),
            1,
            "계산은 성공했어야 함:\n{stdout}"
        );
        assert!(!stdout.contains("[xazz:timing]"), "{stdout}");
        assert!(!stdout.contains("[xazz:result]"), "{stdout}");
        assert!(!stdout.contains("Execution Result"), "{stdout}");
        assert!(!stdout.contains("CSV saved"), "{stdout}");
        assert!(!fixture.0.join("missing-parent").exists());
        assert!(fixture.0.join("occupied").is_dir());
    }
}

#[test]
fn successful_output_contains_expected_csv_and_success_markers() {
    let fixture = Fixture::new("v result = load(\"data.csv\") :: S |> filter(a > 1);");
    let output = fixture.run(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "정상 출력 저장은 성공해야 함:\n{stderr}"
    );
    let csv = std::fs::read_to_string(fixture.0.join("final.csv")).unwrap();
    assert_eq!(csv.lines().collect::<Vec<_>>(), ["a,b", "2,20"]);
    assert_eq!(stdout.matches("[xazz:timing]").count(), 1, "{stdout}");
    assert_eq!(stdout.matches("[xazz:result]").count(), 1, "{stdout}");
    let timing: serde_json::Value = serde_json::from_str(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("[xazz:timing] "))
            .unwrap(),
    )
    .unwrap();
    assert!(
        timing["pipeline_ms"]
            .as_f64()
            .is_some_and(|ms| ms.is_finite() && ms >= 0.0)
    );
    let result: serde_json::Value = serde_json::from_str(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("[xazz:result] "))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["rows"], serde_json::json!([{"a": 2, "b": 20}]));
    assert!(stdout.contains("CSV saved: final.csv"), "{stdout}");
}
