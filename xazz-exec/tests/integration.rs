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
use std::sync::atomic::{AtomicU64, Ordering};

use xazz_exec::run_pipeline;

/// Serializes tests that change the process-global CWD so they don't interfere.
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Monotonic per-process discriminator. The wall clock can be too coarse on
/// some platforms (notably Windows) for two parallel tests to get distinct
/// nanosecond stamps, which previously made them share a temp directory and
/// race on cleanup (flaky module-resolution tests, issue #96). The atomic
/// counter guarantees a unique suffix regardless of clock resolution.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Acquires the CWD lock, recovering from a poisoned mutex so a single test's
/// panic cannot cascade into `PoisonError` for every other CWD-touching test.
fn lock_cwd() -> std::sync::MutexGuard<'static, ()> {
    CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Creates and returns a unique temp folder (cleaned up on process exit).
fn temp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    let unique = format!(
        "xazz_test_{}_{}_{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
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
    let _guard = lock_cwd();
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

// ── issue #55: columnar schema inference for `xazz import` ───────────────────

#[test]
fn infer_columnar_schema_generates_type_block() {
    use polars::prelude::{ParquetWriter, df};
    use xazz_exec::schema_infer::infer_columnar_schema;

    let dir = temp_dir();
    let pq_path = dir.join("data.parquet");

    // Write a parquet file directly with polars — deterministic and independent
    // of run_pipeline/CWD (avoids cross-test contamination on CI).
    let mut df = df!(
        "a" => [1i64, 2i64],
        "b" => [2.5f64, 3.5f64],
        "c" => ["x", "y"],
    )
    .unwrap();
    let file = std::fs::File::create(&pq_path).unwrap();
    ParquetWriter::new(file).finish(&mut df).unwrap();

    let block = infer_columnar_schema(pq_path.to_str().unwrap()).expect("스키마 추론 실패");
    assert!(
        block.contains("type Data = {"),
        "parquet 스키마로 type 블록 생성: {block}"
    );
    assert!(block.contains("a: int"), "int 컬럼 매핑: {block}");
    assert!(block.contains("b: float"), "float 컬럼 매핑: {block}");
    assert!(block.contains("c: string"), "string 컬럼 매핑: {block}");
    assert!(block.contains("v data = load"), "load 구문 생성: {block}");
}

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

    let _guard = lock_cwd();
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

// ── issue #69: module system — import "path.xzz" ─────────────────────────────

/// A main pipeline imports a prep module that defines a shared type + cleaned
/// dataframe, then references both. Proves imports merge declarations into the
/// analysis and execution scope.
#[test]
fn module_import_shared_type_and_pipeline() {
    let dir = temp_dir();
    write_csv(&dir, "pm10,pm25\n80,25\n45,12\n120,40\n");
    std::fs::write(
        dir.join("prep.xzz"),
        "type AQ = { pm10: float, pm25: float };\n\
         v raw = load(\"data.csv\") :: AQ |> dropNull(\"pm10\");\n",
    )
    .unwrap();
    write_xzz(
        &dir.join("data.csv"),
        "import \"prep.xzz\";\n\
         v avg = raw |> groupBy(\"pm10\") |> mean(\"pm25\");",
    );

    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "모듈 import 실행 실패: {:?}", result);
}

/// A cyclic import must fail closed with a clear diagnostic.
#[test]
fn module_cyclic_import_fails_closed() {
    let dir = temp_dir();
    std::fs::write(
        dir.join("a.xzz"),
        "import \"b.xzz\";\ntype A = { a: int };\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("b.xzz"),
        "import \"a.xzz\";\ntype B = { b: int };\n",
    )
    .unwrap();
    write_xzz(
        &dir.join("data.csv"),
        "import \"a.xzz\";\nv p = load(\"data.csv\") :: A;",
    );

    let result = run_in_dir(&dir);
    assert!(result.is_err(), "사이클 import 는 차단되어야 함");
    let msg = format!("{:?}", result);
    assert!(
        msg.contains("cyclic import") || msg.contains("MODULE ERROR"),
        "사이클 진단 메시지: {msg}"
    );
}

// ── v0.23: agg([...]) multi-aggregation + load() sep/header options ──────────

/// Reads a Parquet artifact written by `save()` for content assertions.
fn read_parquet(path: &Path) -> polars::prelude::DataFrame {
    use polars::prelude::{ParquetReader, SerReader};
    let file = std::fs::File::open(path).expect("parquet 열기 실패");
    ParquetReader::new(file)
        .finish()
        .expect("parquet 파싱 실패")
}

/// `agg([...])` after `groupBy` runs as one pass and produces aliased columns.
#[test]
fn agg_list_multi_aggregation_in_one_pass() {
    let dir = temp_dir();
    write_csv(
        &dir,
        "station,pm10\ngangnam,80\ngangnam,40\nseocho,120\nseocho,100\n",
    );
    write_xzz(
        &dir.join("data.csv"),
        "type AQ = { station: string, pm10: float };
         v s = load(\"data.csv\") :: AQ
           |> groupBy(\"station\")
           |> agg([min(\"pm10\"), mean(\"pm10\"), max(\"pm10\")])
           |> save(\"out.parquet\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "agg([...]) 실행 실패: {:?}", result);

    let df = read_parquet(&dir.join("out.parquet"));
    let names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    for expected in ["pm10_min", "pm10_mean", "pm10_max"] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected} 컬럼 없음: {names:?}"
        );
    }
    assert_eq!(df.height(), 2, "station 그룹 2개 기대");
}

/// `load(sep:, header:)` parses a headerless, ';'-separated file end to end.
#[test]
fn load_separator_and_header_options_end_to_end() {
    let dir = temp_dir();
    // No header, ';'-separated — schema fields map by position.
    std::fs::write(dir.join("data.csv"), "gangnam;80\nseocho;120\n").unwrap();
    write_xzz(
        &dir.join("data.csv"),
        "type AQ = { station: string, pm10: float };
         v s = load(\"data.csv\", sep: \";\", header: false) :: AQ
           |> save(\"out.parquet\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "load(sep, header) 실행 실패: {:?}", result);

    let df = read_parquet(&dir.join("out.parquet"));
    let names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(
        names.contains(&"station".to_string()),
        "station 컬럼 없음: {names:?}"
    );
    assert!(
        names.contains(&"pm10".to_string()),
        "pm10 컬럼 없음: {names:?}"
    );
    assert_eq!(df.height(), 2, "2행 기대");
}

/// D3 hyperparameter sweep: list-valued train args train every combination and
/// the best model is used for the downstream predict().
#[test]
fn train_hyperparameter_sweep_end_to_end() {
    let dir = temp_dir();
    write_csv(
        &dir,
        "x1,x2,y\n0,1,0\n1,1,1\n2,0,2\n3,0,3\n4,1,4\n5,1,5\n6,0,6\n7,0,7\n",
    );
    write_xzz(
        &dir.join("data.csv"),
        "type D = { x1: float, x2: float, y: float };
         model M { Dense(4) -> ReLU() -> Dense(1) }
         v data = load(\"data.csv\") :: D;
         v m = data |> train(M, target: \"y\", epochs: [2, 3], lr: [0.05, 0.01], validation_split: 0.25);
         v out = data |> predict(m, as: \"pred\") |> save(\"out.parquet\");",
    );
    let result = run_in_dir(&dir);
    assert!(
        result.is_ok(),
        "하이퍼파라미터 스윕 실행 실패: {:?}",
        result
    );

    let df = read_parquet(&dir.join("out.parquet"));
    assert!(df.column("pred").is_ok(), "예측 컬럼 없음");
    assert_eq!(df.height(), 8, "8행 기대");
}

// ── issue #164: statistical operators ────────────────────────────────────────

/// `skew`, `kurtosis`, `quantile` and `corr` run end to end and produce the
/// expected statistics for a perfectly linear `y = 2x` sample.
#[test]
fn statistical_operators_end_to_end() {
    let dir = temp_dir();
    write_csv(&dir, "x,y\n1,2\n2,4\n3,6\n4,8\n5,10\n");
    write_xzz(
        &dir.join("data.csv"),
        "type P = { x: float, y: float };
         v base = load(\"data.csv\") :: P;
         v sk = base |> skew(\"x\") |> save(\"sk.parquet\");
         v ku = base |> kurtosis(\"x\") |> save(\"ku.parquet\");
         v qn = base |> quantile(\"x\", 0.5) |> save(\"qn.parquet\");
         v co = base |> corr(\"x\", \"y\") |> save(\"co.parquet\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "통계 연산자 실행 실패: {:?}", result);

    let skew = read_parquet(&dir.join("sk.parquet"));
    assert_eq!(skew.height(), 1);
    let s = skew.column("x").unwrap().f64().unwrap().get(0).unwrap();
    assert!(s.abs() < 1e-9, "대칭 표본의 왜도는 0 이어야 함: {s}");

    let kurt = read_parquet(&dir.join("ku.parquet"));
    assert_eq!(kurt.height(), 1);
    let k = kurt.column("x").unwrap().f64().unwrap().get(0).unwrap();
    assert!(k.is_finite(), "첨도는 유한해야 함: {k}");

    let q = read_parquet(&dir.join("qn.parquet"));
    let qv = q.column("x").unwrap().f64().unwrap().get(0).unwrap();
    assert!((qv - 3.0).abs() < 1e-9, "중앙값 분위수는 3 이어야 함: {qv}");

    let c = read_parquet(&dir.join("co.parquet"));
    let cv = c
        .column("x_y_corr")
        .expect("corr 컬럼명 x_y_corr")
        .f64()
        .unwrap()
        .get(0)
        .unwrap();
    assert!(
        (cv - 1.0).abs() < 1e-9,
        "완전 선형 상관은 1 이어야 함: {cv}"
    );
}

/// `histogram("col", bins: N)` replaces the frame with `(bin, count)` rows whose
/// counts partition every finite input value.
#[test]
fn histogram_end_to_end() {
    let dir = temp_dir();
    write_csv(&dir, "x\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    write_xzz(
        &dir.join("data.csv"),
        "type P = { x: float };
         v h = load(\"data.csv\") :: P
           |> histogram(\"x\", bins: 2)
           |> save(\"out.parquet\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "histogram 실행 실패: {:?}", result);

    let df = read_parquet(&dir.join("out.parquet"));
    assert!(df.column("bin").is_ok(), "bin 컬럼 없음");
    assert!(df.column("n").is_ok(), "n 컬럼 없음");
    assert_eq!(df.height(), 2, "bins: 2 → 2행 기대");
    let total: u32 = df
        .column("n")
        .unwrap()
        .u32()
        .unwrap()
        .into_no_null_iter()
        .sum();
    assert_eq!(total, 10, "모든 값이 버킷에 포함되어야 함");
}

/// `outlierIqr("col", factor: 1.5)` drops values outside `[Q1 - 1.5·IQR, Q3 + 1.5·IQR]`.
#[test]
fn outlier_iqr_filters_outliers_end_to_end() {
    let dir = temp_dir();
    write_csv(&dir, "x\n1\n2\n3\n4\n100\n");
    write_xzz(
        &dir.join("data.csv"),
        "type P = { x: float };
         v o = load(\"data.csv\") :: P
           |> outlierIqr(\"x\", factor: 1.5)
           |> save(\"out.parquet\");",
    );
    let result = run_in_dir(&dir);
    assert!(result.is_ok(), "outlierIqr 실행 실패: {:?}", result);

    let df = read_parquet(&dir.join("out.parquet"));
    assert_eq!(df.height(), 4, "이상치 100 이 제거되어 4행 기대");
}
