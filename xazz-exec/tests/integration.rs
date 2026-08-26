// xazz-exec 통합 테스트 — 실제 .xzz 스크립트를 Polars 런타임으로 실행한다.
//
// 시스템 임시 디렉터리에 CSV 와 .xzz 스크립트를 생성하고 run_pipeline() 을
// 호출해 전체 Lexer → Parser → TypeChecker → Polars 실행 흐름을 검증한다.
//
// ⚠️ run_pipeline() 은 stdout/stderr 로 로그를 출력한다. 테스트는 반환
//    Result 만 검증한다. 임시 폴더는 std 만으로 관리한다 (외부 의존성 없음).

use std::path::{Path, PathBuf};

use xazz_exec::run_pipeline;

/// 임시 폴더를 하나 만들어 반환한다 (프로세스 종료 시 정리).
fn temp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    let unique = format!(
        "xazz_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = base.join(unique);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 임시 폴더에 CSV 파일을 생성하고 경로를 반환한다.
fn write_csv(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("data.csv");
    std::fs::write(&path, contents).unwrap();
    path
}

/// CSV 옆에 .xzz 스크립트를 작성하고 경로를 반환한다.
fn write_xzz(csv: &Path, script: &str) -> PathBuf {
    let xzz_path = csv.with_file_name("pipeline.xzz");
    std::fs::write(&xzz_path, script).unwrap();
    xzz_path
}

fn csv_abs(csv: &Path) -> String {
    csv.to_string_lossy().replace('\\', "/")
}

#[test]
fn run_valid_preprocessing_pipeline() {
    let dir = temp_dir();
    let csv = write_csv(
        &dir,
        "station,pm10,pm25\ngangnam,80,25\ngangnam,45,12\nseocho,120,40\n",
    );
    let abs = csv_abs(&csv);
    let xzz = write_xzz(
        &csv,
        &format!(
            "type AQ = {{ station: string, pm10: float, pm25: float }};
             v a = load(\"{abs}\") :: AQ
               |> filter(pm10 > 10)
               |> groupBy(\"station\")
               |> mean(\"pm10\")
               |> orderBy(\"pm10\", desc: true);"
        ),
    );

    let result = run_pipeline(xzz.to_str().unwrap(), false, None);
    assert!(result.is_ok(), "유효한 파이프라인 실행 실패: {:?}", result);
}

#[test]
fn run_empty_pipeline_collects_rows() {
    let dir = temp_dir();
    let csv = write_csv(&dir, "a,b\n1,2\n3,4\n5,6\n");
    let abs = csv_abs(&csv);
    let xzz = write_xzz(
        &csv,
        &format!(
            "type S = {{ a: int, b: int }};
             v p = load(\"{abs}\") :: S;"
        ),
    );
    let result = run_pipeline(xzz.to_str().unwrap(), false, None);
    assert!(result.is_ok(), "빈 파이프라인 실행 실패: {:?}", result);
}

#[test]
fn run_join_between_two_pipelines() {
    let dir = temp_dir();
    let csv = write_csv(&dir, "id,val\n1,10\n2,20\n3,30\n");
    let abs = csv_abs(&csv);
    let xzz = write_xzz(
        &csv,
        &format!(
            "type T = {{ id: int, val: int }};
             v left = load(\"{abs}\") :: T;
             v right = left |> filter(val > 15);
             v joined = left |> join(right, left_on: [\"id\"], right_on: [\"id\"], how: \"inner\");"
        ),
    );
    let result = run_pipeline(xzz.to_str().unwrap(), false, None);
    assert!(result.is_ok(), "join 파이프라인 실행 실패: {:?}", result);
}
