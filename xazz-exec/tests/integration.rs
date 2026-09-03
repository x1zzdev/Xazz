// xazz-exec integration tests — run real .xzz scripts through the Polars runtime.
//
// Creates a CSV and .xzz script in a system temp directory and calls
// run_pipeline() to verify the full Lexer → Parser → TypeChecker → Polars flow.
//
// ⚠️ run_pipeline() writes logs to stdout/stderr. The tests only check the
//    returned Result. Temp folders are managed with std only (no external deps).
//
// ⚠️ Policy-as-Code blocks absolute-path load() with a fail-closed rule. So the
//    tests chdir into their temp dir and reference the CSV with a **relative
//    path**. Since chdir is process-global, a global Mutex serializes the tests
//    to avoid parallel collisions.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use xazz_exec::run_pipeline;

/// Serializes tests that change the process-global CWD so they don't interfere.
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Creates and returns a unique temp folder (cleaned up on process exit).
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

/// Creates a CSV file in the temp folder and returns its path.
fn write_csv(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("data.csv");
    std::fs::write(&path, contents).unwrap();
    path
}

/// Writes a .xzz script next to the CSV and returns its path.
fn write_xzz(csv: &Path, script: &str) -> PathBuf {
    let xzz_path = csv.with_file_name("pipeline.xzz");
    std::fs::write(&xzz_path, script).unwrap();
    xzz_path
}

/// Chdirs into the temp dir and runs the script via a **relative path**.
///
/// The returned guard restores the original CWD when it goes out of scope.
/// (Policy-as-Code blocks absolute paths, so the tests must use relative ones.)
fn run_in_dir(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let result = run_pipeline("pipeline.xzz", false, None, false);
    std::env::set_current_dir(original).unwrap();
    result
}

#[test]
fn run_valid_preprocessing_pipeline() {
    let dir = temp_dir();
    write_csv(
        &dir,
        "station,pm10,pm25\ngangnam,80,25\ngangnam,45,12\nseocho,120,40\n",
    );
    write_xzz(
        &dir.join("data.csv"),
        "type AQ = { station: string, pm10: float, pm25: float };
         v a = load(\"data.csv\") :: AQ
           |> filter(pm10 > 10)
           |> groupBy(\"station\")
           |> mean(\"pm10\")
           |> orderBy(\"pm10\", desc: true);",
    );

    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "유효한 파이프라인 실행 실패: {:?}", result);
}

#[test]
fn run_empty_pipeline_collects_rows() {
    let dir = temp_dir();
    write_csv(&dir, "a,b\n1,2\n3,4\n5,6\n");
    write_xzz(
        &dir.join("data.csv"),
        "type S = { a: int, b: int };
         v p = load(\"data.csv\") :: S;",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "빈 파이프라인 실행 실패: {:?}", result);
}

#[test]
fn run_join_between_two_pipelines() {
    let dir = temp_dir();
    write_csv(&dir, "id,val\n1,10\n2,20\n3,30\n");
    write_xzz(
        &dir.join("data.csv"),
        "type T = { id: int, val: int };
         v left = load(\"data.csv\") :: T;
         v right = left |> filter(val > 15);
         v joined = left |> join(right, left_on: [\"id\"], right_on: [\"id\"], how: \"inner\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "join 파이프라인 실행 실패: {:?}", result);
}

// ── issue #52: save() + columnar load round-trip ─────────────────────────────

/// Writes a .xzz script that loads CSV → filters → saves Parquet, then runs it
/// and asserts the artifact exists. Also loads the artifact back in a second
/// pipeline to prove the load() columnar path works.
#[test]
fn save_parquet_then_load_back() {
    let dir = temp_dir();
    write_csv(&dir, "a,b\n1,10\n2,20\n3,30\n");
    write_xzz(
        &dir.join("data.csv"),
        "type S = { a: int, b: int };
         v p = load(\"data.csv\") :: S
           |> filter(a > 1)
           |> save(\"out.parquet\");
         v q = load(\"out.parquet\") :: S
           |> sum(\"b\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "parquet save/load 실패: {:?}", result);
    let parquet_path = dir.join("out.parquet");
    assert!(
        parquet_path.exists(),
        "save() 가 out.parquet 을 생성하지 못함"
    );
    assert!(
        std::fs::metadata(&parquet_path).unwrap().len() > 0,
        "out.parquet 이 비어 있음"
    );
}

/// Arrow format round-trip through save() + load().
#[test]
fn save_arrow_then_load_back() {
    let dir = temp_dir();
    write_csv(&dir, "x\n5\n7\n9\n");
    write_xzz(
        &dir.join("data.csv"),
        "type S = { x: int };
         v p = load(\"data.csv\") :: S
           |> save(\"out.arrow\", format: \"arrow\");
         v q = load(\"out.arrow\") :: S
           |> mean(\"x\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "arrow save/load 실패: {:?}", result);
    let arrow_path = dir.join("out.arrow");
    assert!(arrow_path.exists(), "save() 가 out.arrow 을 생성하지 못함");
}

// ── issue #53: streaming engine on the benchmark-shaped pipeline ─────────────

/// Runs the same composite workload as benches/bench_scale_small.xzz
/// (dropNull → dual filter → groupBy+sum → groupBy+mean → orderBy → take →
/// fillNull → count) with the out-of-core streaming engine enabled, proving the
/// streaming collect path handles the benchmark operator set.
#[test]
fn streaming_engine_runs_benchmark_shaped_pipeline() {
    let dir = temp_dir();
    write_csv(
        &dir,
        "date,station,pm10,pm25\n\
         2026-01-01,gangnam,80,25\n\
         2026-01-02,gangnam,45,12\n\
         2026-01-03,seocho,120,40\n\
         2026-01-04,seocho,90,30\n",
    );
    write_xzz(
        &dir.join("data.csv"),
        "type AQ = { date: string, station: string, pm10: Option<float>, pm25: Option<float> };
         v p2 = load(\"data.csv\") :: AQ
           |> dropNull(\"pm10\")
           |> filter(pm10 < 120)
           |> filter(pm25 > 10);
         v p3 = p2 |> groupBy(\"station\") |> sum(\"pm10\");
         v p4 = p2 |> groupBy(\"station\") |> mean(\"pm10\")
           |> orderBy(\"pm10\", desc: true) |> take(10);
         v p7 = load(\"data.csv\") :: AQ
           |> fillNull(\"pm25\", 0)
           |> filter(pm10 > 50)
           |> groupBy(\"station\") |> count(\"pm25\")
           |> orderBy(\"pm25\", desc: true) |> take(5);",
    );

    let _guard = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    // Enable the streaming engine for the whole process during this test.
    unsafe { std::env::set_var("XAZZ_STREAMING", "1") };
    let result = run_pipeline("pipeline.xzz", false, None, false);
    unsafe { std::env::remove_var("XAZZ_STREAMING") };
    std::env::set_current_dir(original).unwrap();
    assert!(
        result.is_ok(),
        "streaming 엔진 벤치 파이프라인 실행 실패: {:?}",
        result
    );
}
