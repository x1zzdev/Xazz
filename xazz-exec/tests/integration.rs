// xazz-exec 통합 테스트 — 실제 .xzz 스크립트를 Polars 런타임으로 실행한다.
//
// 시스템 임시 디렉터리에 CSV 와 .xzz 스크립트를 생성하고 run_pipeline() 을
// 호출해 전체 Lexer → Parser → TypeChecker → Polars 실행 흐름을 검증한다.
//
// ⚠️ run_pipeline() 은 stdout/stderr 로 로그를 출력한다. 테스트는 반환
//    Result 만 검증한다. 임시 폴더는 std 만으로 관리한다 (외부 의존성 없음).
//
// ⚠️ Policy-as-Code 는 절대 경로 load() 를 fail-closed 로 차단한다. 따라서
//    테스트는 임시 디렉터리로 chdir 한 뒤 **상대 경로** 로 CSV 를 참조한다.
//    chdir 는 프로세스 전역 상태이므로, 병렬 테스트 충돌을 막기 위해 전역
//    Mutex 로 직렬화한다.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use xazz_exec::run_pipeline;

/// chdir 를 프로세스 전역으로 바꾸는 테스트들이 서로 간섭하지 않도록 직렬화.
static CWD_LOCK: Mutex<()> = Mutex::new(());

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

/// 임시 디렉터리로 chdir 하고 스크립트를 **상대 경로** 로 실행한다.
///
/// 반환한 가드는 스코프를 벗어나면 원래 CWD 로 복원한다. (절대 경로는
/// Policy-as-Code 가 차단하므로 테스트는 상대 경로를 써야 한다.)
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
