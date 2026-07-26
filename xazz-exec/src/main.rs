// xazz-exec/src/main.rs
//
// xazz 실행 엔진 바이너리 — Polars LazyFrame 런타임
//
// ⚠️  이 바이너리는 Polars/encoding_rs/tokio/rayon을 정적 링크한다.
//     xazz CLI는 절대 이 크레이트를 직접 링크하지 않는다.
//     xazz-runner 가 이 바이너리를 서브프로세스로 스폰한다.
//
// 사용법:
//   xazz-exec <file.xzz> [--verbose] [--output <path.csv>]
//   xazz-exec <file.csv> [--verbose]   (CSV 직접 입력 → 벤치마크 파이프라인)
//
// 통신 프로토콜:
//   - 입력:  CLI args + (선택적) stdin JSON
//   - 출력:  stdout (결과 테이블, [xazz:result] JSON 마커, 차트 마커)
//   - 에러:  stderr
//   - 종료 코드: 0 = 성공, 1 = 실패

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!(
            "[xazz-exec] 사용법: xazz-exec <file.xzz|file.csv> [--verbose] [--output <path.csv>]"
        );
        std::process::exit(1);
    }

    let input_path = &args[1];
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");

    // --output <path> 파싱
    let output_csv: Option<String> = args
        .windows(2)
        .find(|w| w[0] == "--output" || w[0] == "-o")
        .map(|w| w[1].clone());

    // ── CSV 직접 입력 → 벤치마크 파이프라인 자동 생성 ───────────────────────
    if input_path.to_lowercase().ends_with(".csv") {
        run_csv_benchmark(input_path, verbose);
        return;
    }

    // ── .xzz 파일 실행 ──────────────────────────────────────────────────────
    if let Err(e) = xazz_exec::run_pipeline(input_path, verbose, output_csv.as_deref()) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

/// CSV 경로를 받아 벤치마크용 .xzz 스크립트를 임시 파일로 생성한 뒤
/// run_pipeline() 으로 실행하고 임시 파일을 정리한다.
fn run_csv_benchmark(csv_path: &str, verbose: bool) {
    let posix_path = csv_path.replace('\\', "/");

    let xzz_source = format!(
        r#"// xazzLang Benchmark Pipeline — auto-generated from CSV input
type AirQuality = {{
  date: string,
  station: string,
  pm10: Option<float>,
  pm25: Option<float>,
}};

v raw = load("{posix_path}") :: AirQuality
  |> select([date, station, pm10, pm25]);

v cleaned = raw
  |> dropNull("pm10")
  |> filter(col("pm10") < 120)
  |> filter(col("pm25") > 10);

v by_station = cleaned
  |> groupBy("station")
  |> sum("pm10");

v top10_mean = cleaned
  |> groupBy("station")
  |> mean("pm10")
  |> orderBy("pm10", desc: true)
  |> take(10);

v filled = raw
  |> fillNull("pm25", 0)
  |> filter(col("pm10") > 50)
  |> groupBy("station")
  |> count("pm25")
  |> orderBy("pm25", desc: true)
  |> take(5);
"#,
        posix_path = posix_path
    );

    let tmp_xzz_path = if let Some(parent) = Path::new(csv_path).parent() {
        let stem = Path::new(csv_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("benchmark");
        parent
            .join(format!("_{}_bench.xzz", stem))
            .to_string_lossy()
            .to_string()
    } else {
        format!("_{}_bench.xzz", csv_path)
    };

    if let Err(e) = std::fs::write(&tmp_xzz_path, &xzz_source) {
        eprintln!(
            "[xazz-exec] ERROR: 임시 .xzz 파일 생성 실패: {} — {}",
            tmp_xzz_path, e
        );
        std::process::exit(1);
    }

    let result = xazz_exec::run_pipeline(&tmp_xzz_path, verbose, None);
    let _ = std::fs::remove_file(&tmp_xzz_path);

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
