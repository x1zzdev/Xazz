# GPU and ONNX backends — build & run guide

`xazz-exec` can run model training and inference on several compute backends.
CPU (`burn-ndarray`, pure Rust) is always compiled and is the fallback; the
GPU/ONNX providers are Cargo **features** that pull in their native SDK. This
page explains which feature to build, what each host needs, and how to select a
device at runtime.

> Acceptance evidence (which provider was verified on which hardware) lives in
> [docs/design/gpu-backend-acceptance.md](design/gpu-backend-acceptance.md).
> Measurement methodology and cache/chunk design live in
> [docs/design/gpu-backend-perf-log.md](design/gpu-backend-perf-log.md).

## 1. Feature matrix

Features are declared in [`xazz-exec/Cargo.toml`](../xazz-exec/Cargo.toml).

| Feature | `XAZZ_BACKEND` | Engine | Host requirement |
|---------|----------------|--------|------------------|
| *(none, default)* | `cpu` | `burn-ndarray` | none — always compiled |
| `wgpu` | `wgpu` | `burn-wgpu` (WebGPU/CubeCL) | Vulkan / Metal / DX12 device; no external SDK |
| `cuda` | `cuda` | `burn-cuda` (native CubeCL CUDA) | NVIDIA GPU + CUDA driver; no LibTorch/SDK |
| `onnx` | `onnx` | ONNX Runtime (CPU EP) | none — prebuilt runtime is downloaded |
| `onnx-cuda` | `onnx` | ONNX Runtime + CUDA EP | NVIDIA GPU + CUDA |
| `onnx-tensorrt` | `onnx` | ONNX Runtime + TensorRT EP | NVIDIA GPU + TensorRT |
| `onnx-directml` | `onnx` | ONNX Runtime + DirectML EP | Windows GPU |
| `onnx-coreml` | `onnx` | ONNX Runtime + CoreML EP | macOS |

Each `onnx-*` feature enables `onnx` plus the matching `ort` GPU feature, so
`ort`'s `download-binaries` fetches a GPU-enabled ONNX Runtime instead of the
CPU build. `onnx` alone runs on the CPU execution provider.

## 2. Build

Build **`xazz-exec`** with the feature — it is the binary that owns the ML
runtime. `xazz` and `xazz-runner` stay lightweight and never link Polars/Burn.

```bash
# WebGPU (no SDK required)
cargo build --release -p xazz-exec --features wgpu

# NVIDIA via native CubeCL CUDA
cargo build --release -p xazz-exec --features cuda

# ONNX Runtime, CPU execution provider
cargo build --release -p xazz-exec --features onnx

# ONNX Runtime with a GPU execution provider
cargo build --release -p xazz-exec --features onnx-cuda
cargo build --release -p xazz-exec --features onnx-directml
```

Then build the rest of the demo binaries as usual (see
[DEMO_GUIDE.md](../DEMO_GUIDE.md#step-1--build-the-binaries)) and keep
`xazz`, `xazz-runner`, and `xazz-exec` in the same directory.

> **CUDA backend is pure Rust.** The `cuda` feature uses `burn-cuda` (native
> CubeCL), which JIT-compiles kernels at runtime and needs no LibTorch, system
> SDK, or MSVC toolchain — only a working NVIDIA driver at run time. This is the
> same CubeCL stack as `burn-wgpu` and matches the upstream Burn 0.22 direction
> (CubeCL CUDA, graph replay, LLVM GPU backends). See [issue #62](https://github.com/x1zzdev/Xazz/issues/62).

### Platform notes

- **Windows.** Every `onnx*` feature requires the **MSVC toolchain**
  (`stable-x86_64-pc-windows-msvc` + VS Build Tools, "Desktop development with
  C++") because ONNX Runtime has no windows-gnu prebuilt, so `build.rs` blocks
  those features on GNU up front. `wgpu` and `cuda` work on either toolchain.
  See [gpu-backend-acceptance.md §6](design/gpu-backend-acceptance.md).
- **Linux.** `wgpu` uses Vulkan (Mesa/lavapipe is enough for CI). `cuda` needs
  an NVIDIA driver (CubeCL kernels are JIT-compiled); `onnx` uses the prebuilt
  runtime.
- **macOS.** `wgpu` uses Metal; `onnx-coreml` targets the Apple Neural Engine /
  CoreML.

## 3. Runtime selection

### `XAZZ_BACKEND`

Selects the provider. Default is `cpu`. Aliases are accepted (case-insensitive).

| Value | Aliases | Provider |
|-------|---------|----------|
| `cpu` | `ndarray`, `burn`, `burn-ndarray`, empty | CPU |
| `wgpu` | `gpu`, `webgpu`, `burn-wgpu` | WebGPU |
| `cuda` | `nvidia`, `burn-cuda` | native CubeCL CUDA |
| `onnx` | `onnxruntime`, `ort` | ONNX Runtime |

If the requested backend was **not compiled into the binary**, or the device is
unavailable, the runtime **falls back to CPU with an explicit warning** — it
never switches device silently. An unrecognised value also falls back to CPU.

### `XAZZ_DEVICE` (unified selector)

One grammar covers the WebGPU adapter, the CUDA device index, and the ONNX
execution provider. It takes precedence over the per-provider variables below.

```
auto | default | cpu | coreml
dgpu[:N] | discrete[:N]      # n-th discrete GPU
igpu[:N] | integrated[:N]    # n-th integrated GPU
vgpu[:N] | virtual[:N]       # n-th virtual GPU
cuda[:N]                     # CUDA device n
tensorrt[:N] | trt[:N]       # TensorRT device n
directml[:N] | dml[:N]       # DirectML device n
```

`N` defaults to `0`. Unrecognised values fail closed with a diagnostic.

### Per-provider fallbacks (legacy)

| Variable | Provider | Values |
|----------|----------|--------|
| `XAZZ_WGPU_DEVICE` | WebGPU | `default`/`auto`/`cpu`, `dgpu[:N]`, `igpu[:N]`, `vgpu[:N]` |
| `XAZZ_CUDA_DEVICE` | CUDA | device index (default `0`) |
| `XAZZ_ORT_EP` | ONNX | `auto` or a comma list of `cpu,cuda,tensorrt,directml,coreml` (order preserved) |
| `XAZZ_ORT_DEVICE` | ONNX CUDA/TensorRT/DirectML | device index (default `0`) |

`XAZZ_ORT_EP=auto` registers every GPU EP compiled into the build (falling back
to CPU). An explicit list is **fail-closed**: if a listed EP is unavailable, the
run errors instead of silently dropping it.

### Examples

```bash
# WebGPU on the discrete GPU (e.g. RTX 4070)
XAZZ_BACKEND=wgpu XAZZ_DEVICE=dgpu:0 xazz run model.xzz

# WebGPU on the integrated GPU (e.g. Intel Arc)
XAZZ_BACKEND=wgpu XAZZ_DEVICE=igpu:0 xazz run model.xzz

# CUDA device 1
XAZZ_BACKEND=cuda XAZZ_DEVICE=cuda:1 xazz run model.xzz

# ONNX Runtime on the CPU execution provider
XAZZ_BACKEND=onnx xazz run model.xzz

# ONNX Runtime, explicitly CUDA then CPU (fail-closed if CUDA is missing)
XAZZ_BACKEND=onnx XAZZ_ORT_EP=cuda,cpu xazz run model.xzz
```

### Inference tuning

| Variable | Default | Meaning |
|----------|---------|---------|
| `XAZZ_INFER_CACHE_SLOTS` | `4` | LRU slots for loaded models/ONNX sessions |
| `XAZZ_INFER_CHUNK` | `4096` | Rows per inference upload (`0` disables chunking) |

## 4. Verify an acceptance test

The GPU/ONNX acceptance tests are `#[ignore]`d behind their feature. Run them on
a host with the hardware (they compare against the CPU reference):

```bash
cargo test --release -p xazz-exec --features wgpu  -- --ignored
cargo test --release -p xazz-exec --features cuda  -- --ignored
cargo test --release -p xazz-exec --features onnx  -- --ignored
```

Use `--release`: the native GPU dependencies are built/optimized differently in
debug and GPU runs are impractically slow there.

## 5. Current verification status

| Backend | Status |
|---------|--------|
| CPU | Reference implementation — always available |
| `wgpu` | **Verified** on Windows 11 / RTX 4070 (dGPU) and Intel Arc (iGPU); `XAZZ_DEVICE` adapter pinning confirmed |
| `cuda` | Native CubeCL (`burn-cuda`) — implementation complete; real-hardware acceptance pending a CUDA-driver host ([#103](https://github.com/x1zzdev/Xazz/issues/103), [#236](https://github.com/x1zzdev/Xazz/issues/236)) |
| `onnx` (CPU EP) | Implementation complete; real-hardware acceptance pending a standard (MSVC) toolchain ([#103](https://github.com/x1zzdev/Xazz/issues/103)) |
| `onnx-coreml` | Pending macOS hardware |
