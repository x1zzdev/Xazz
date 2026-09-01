// xazz-exec/src/main.rs
//
// xazz execution-engine binary — Polars LazyFrame runtime
//
// ⚠️  This binary statically links Polars/encoding_rs/tokio/rayon.
//     The xazz CLI never links this crate directly.
//     xazz-runner spawns this binary as a subprocess.
//
// Usage:
//   xazz-exec <file.xzz> [--verbose] [--output <path.csv>]
//   xazz-exec <file.csv> [--verbose]   (direct CSV input → benchmark pipeline)
//
// Communication protocol:
//   - input:  CLI args + (optional) stdin JSON
//   - output: stdout (result table, [xazz:result] JSON marker, chart markers)
//   - errors: stderr
//   - exit code: 0 = success, 1 = failure

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "[xazz-exec] usage: xazz-exec <file.xzz|file.csv> [--verbose] [--output <path.csv>] [--opt]";

    // ── Helper flags ────────────────────────────────────────────────────────
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("xazz-exec {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{usage}");
        println!("[xazz-exec] helpers: --version | --help");
        return;
    }

    if args.len() < 2 {
        eprintln!("{usage}");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let optimize = args.iter().any(|a| a == "--opt" || a == "--optimize");

    // parse --output <path>
    let output_csv: Option<String> = args
        .windows(2)
        .find(|w| w[0] == "--output" || w[0] == "-o")
        .map(|w| w[1].clone());

    // ── Direct CSV input → auto-generate benchmark pipeline ─────────────────
    if input_path.to_lowercase().ends_with(".csv") {
        run_csv_benchmark(input_path, verbose);
        return;
    }

    // ── Run .xzz file ───────────────────────────────────────────────────────
    if let Err(e) = xazz_exec::run_pipeline(input_path, verbose, output_csv.as_deref(), optimize) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

/// Creates a temporary benchmark .xzz script from a CSV path, runs it through
/// run_pipeline(), and cleans up the temporary file.
fn run_csv_benchmark(csv_path: &str, verbose: bool) {
    let posix_path = csv_path.replace('\\', "/");
    let stem_sanitized: String = std::path::Path::new(csv_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            s.chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect()
        })
        .unwrap_or_else(|| "benchmark".to_string());

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

    // Write to the system temp dir with a process-unique name.
    // (A deterministic name or placing it next to the input CSV could cause
    // races, leaks, or read-only failures.)
    let unique = format!(
        "xazz_bench_{}_{}_{}.xzz",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        stem_sanitized
    );
    let tmp_xzz_path = std::env::temp_dir().join(&unique);

    // RAII guard — removes the temp file regardless of success/failure.
    struct TempFileGuard(std::path::PathBuf);
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    if let Err(e) = std::fs::write(&tmp_xzz_path, &xzz_source) {
        eprintln!(
            "[xazz-exec] ERROR: failed to write temporary .xzz file: {} — {}",
            tmp_xzz_path.display(),
            e
        );
        std::process::exit(1);
    }
    let _guard = TempFileGuard(tmp_xzz_path.clone());

    let result =
        xazz_exec::run_pipeline(&tmp_xzz_path.to_str().unwrap_or(""), verbose, None, false);
    // The guard drops and removes the temp file here.

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
