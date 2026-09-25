//! Build-time guard for the Windows GPU backends (issue #103).
//!
//! `torch-sys` links against MSVC-ABI LibTorch, and `ort-sys` publishes no
//! prebuilt runtime for `x86_64-pc-windows-gnu`. Building the `cuda`/`onnx*`
//! features with the GNU toolchain therefore cannot succeed; fail fast with an
//! actionable message instead of a long stream of C++/download errors.
//!
//! See `docs/design/gpu-backend-acceptance.md` §4–§6.

use std::env;

/// `(cargo feature, CARGO_FEATURE_* env var)` pairs that are MSVC-only on Windows.
const FEATURES: &[(&str, &str)] = &[
    ("cuda", "CARGO_FEATURE_CUDA"),
    ("onnx", "CARGO_FEATURE_ONNX"),
    ("onnx-cuda", "CARGO_FEATURE_ONNX_CUDA"),
    ("onnx-tensorrt", "CARGO_FEATURE_ONNX_TENSORRT"),
    ("onnx-directml", "CARGO_FEATURE_ONNX_DIRECTML"),
    ("onnx-coreml", "CARGO_FEATURE_ONNX_COREML"),
];

fn main() {
    for (_, var) in FEATURES {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-env-changed=XAZZ_ALLOW_WINDOWS_GNU_GPU");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.ends_with("windows-gnu") {
        return;
    }

    let requested: Vec<&str> = FEATURES
        .iter()
        .filter(|(_, var)| env::var_os(var).is_some())
        .map(|(name, _)| *name)
        .collect();
    if requested.is_empty() {
        return;
    }

    let list = requested.join(",");
    if env::var_os("XAZZ_ALLOW_WINDOWS_GNU_GPU").is_some() {
        println!(
            "cargo:warning=[xazz] {list} on {target} is unsupported (MSVC-only); \
             continuing because XAZZ_ALLOW_WINDOWS_GNU_GPU is set"
        );
        return;
    }

    // Emit as warnings so the guidance survives even if the panic body is elided.
    for line in [
        format!("[xazz] GPU feature(s) `{list}` cannot be built for {target}."),
        "CUDA/ONNX on Windows require the MSVC toolchain: LibTorch is MSVC-ABI and \
         ONNX Runtime ships no windows-gnu prebuilt."
            .to_string(),
        "Fix (Windows):".to_string(),
        "  rustup toolchain install stable-x86_64-pc-windows-msvc".to_string(),
        "  rustup default stable-x86_64-pc-windows-msvc".to_string(),
        "  # install VS Build Tools with \"Desktop development with C++\" if missing".to_string(),
        format!("  cargo test --release -p xazz-exec --features {list} -- --ignored --nocapture"),
        "Bypass (unsupported): set XAZZ_ALLOW_WINDOWS_GNU_GPU=1".to_string(),
        "See docs/design/gpu-backend-acceptance.md §4-§6.".to_string(),
    ] {
        println!("cargo:warning={line}");
    }

    panic!(
        "xazz-exec feature(s) `{list}` require the MSVC toolchain on {target}; \
         see the cargo warnings above (or docs/design/gpu-backend-acceptance.md §4-§6)"
    );
}
