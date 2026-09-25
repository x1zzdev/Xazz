# Contributing to Xazz

Thank you for your interest in Xazz.

## Welcome

Thanks for your interest in Xazz! Whether you want to report a bug, propose a feature, join a discussion, or open a pull request, we're glad to have you.

| Contribution type | Status |
|-------------------|--------|
| [Discussions](https://github.com/x1zzdev/Xazz/discussions) (questions, ideas, projects) | Open |
| [Issues](https://github.com/x1zzdev/Xazz/issues) (reproducible bugs, agreed work) | Open |
| Pull Requests | Open |

For help, use [Q&A](https://github.com/x1zzdev/Xazz/discussions/categories/q-a). Share proposals in [Ideas](https://github.com/x1zzdev/Xazz/discussions/categories/ideas) and projects in [Show and tell](https://github.com/x1zzdev/Xazz/discussions/categories/show-and-tell). For a reproducible bug, open a GitHub Issue. Report vulnerabilities privately through [Security advisories](https://github.com/x1zzdev/Xazz/security/advisories/new).

---

## Project Overview

Xazz is a Rust-based DSL compiler platform. The workspace is structured as follows:

```
Xazz/
├── src/                    xazz CLI binary (lightweight — no Polars/Tokio)
├── xazz-core/              Shared AST / Token / Error types
├── xazz-compiler/          Lexer, Parser, Codegen, Emitter
├── xazz-exec/              Polars + Burn execution engine (isolated crate)
├── xazz-runner/            Execution binary (spawned by CLI as subprocess)
├── xazz-server/            REST API server (standalone, powers the visual IDE)
├── visual-ide/             Node-based web IDE (React + @xyflow/react)
├── docs/                   All documentation
│   ├── ARCHITECTURE.md / WORKSPACE.md / result_report.md
│   ├── design/             Product & UX design artifacts (incl. screenshots)
│   ├── design-system/      UI design system
│   ├── design-evidence/    Design evidence records (JSON)
│   ├── spec/               Product spec and discovery notes
│   └── assets/             Screenshots used by README
├── benches/                Benchmark scripts and results
└── examples/               Example .xzz scripts and CSV data
```

Key constraint: **the `xazz` CLI binary must never link Polars or Tokio.** All Polars execution is delegated to `xazz-runner` via subprocess.

---

## Local Build

### Prerequisites

- Rust stable toolchain ([rustup.rs](https://rustup.rs))
- Git

### Build

```bash
git clone https://github.com/x1zzdev/Xazz.git
cd Xazz

# Build CLI binary only (lightweight, no Polars)
cargo build --release -p xazz

# Build execution engine (includes Polars — takes longer)
cargo build --release -p xazz-runner

# Build entire workspace
cargo build --release
```

Binaries are produced in `target/release/`. For `xazz run` to work, both `xazz` and `xazz-runner` must be in the same directory.

### Run a pipeline

```bash
# From target/release/ (or add to PATH)
./xazz run examples/poc_correct.xzz
```

### Verify compiler only (no execution engine needed)

```bash
./xazz emit rust examples/poc_correct.xzz
```

### Run tests

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check --all-features  # requires cargo-deny locally; CI runs its action
```

CI runs the Rust checks in that order, then runs the license and dependency policy check. To test only the crate you changed, use the matching command below; run the full checks before requesting review.

| Crate | Focused test | Note |
|---|---|---|
| `xazz` (CLI) | `cargo test -p xazz` | CLI commands and import |
| `xazz-core` | `cargo test -p xazz-core` | Shared types |
| `xazz-compiler` | `cargo test -p xazz-compiler` | Parser, checker, and emitter |
| `xazz-exec` | `cargo test -p xazz-exec` | Builds the heavier Polars/Burn engine |
| `xazz-runner` | `cargo test -p xazz-runner` | Execution subprocess |
| `xazz-server` | `cargo test -p xazz-server` | HTTP and policy routes |
| `xazz-lsp` | `cargo test -p xazz-lsp` | Language server |

For Visual IDE changes, run `npm ci` and the `test:contract`, `test:stdout`, `test:contrast`, and `test:e2e` scripts from `visual-ide/`, as in `.github/workflows/ci.yml`.

### Optional GPU backends

`xazz-exec` ships CPU-only by default; GPU providers are opt-in cargo features:

| Feature | Provider | Acceptance (gated, `#[ignore]`) |
|---|---|---|
| `wgpu` | `burn-wgpu` (no SDK needed) | `cargo test --release -p xazz-exec --features wgpu -- --ignored --nocapture` |
| `cuda` | `burn-tch` / LibTorch | `TORCH_CUDA_VERSION=cu128 cargo test --release -p xazz-exec --features cuda -- --ignored --nocapture` |
| `onnx` | ONNX Runtime (`ort`) | `cargo test --release -p xazz-exec --features onnx -- --ignored --nocapture` |

- Use `--release` for GPU feature tests: under `windows-gnu` the debug test binary can exceed the 4 GB PE limit.
- **Windows: `cuda`/`onnx*` require the MSVC toolchain.** LibTorch is MSVC-ABI and ONNX Runtime has no `windows-gnu` prebuilt, so `build.rs` fails fast unless you install `stable-x86_64-pc-windows-msvc` + VS Build Tools ("Desktop development with C++"). Set `XAZZ_ALLOW_WINDOWS_GNU_GPU=1` only to bypass the guard (unsupported). See `docs/design/gpu-backend-acceptance.md` §4–§6.

---

## Contribution path

Every change follows the same path:

1. **Discuss / file** — ask in [Discussions](https://github.com/x1zzdev/Xazz/discussions) or open an issue. Reproducible bugs and agreed work become issues.
2. **Pick** — grab a `good first issue` / `help wanted` item, or comment on an issue to claim it.
3. **PR** — branch, implement with a test, and run the gates in [Run tests](#run-tests). Draft PRs are welcome early.
4. **Review** — a maintainer and CI review. Address feedback with new commits rather than force-pushing over review history.
5. **Merge** — a maintainer merges once CI is green and the review is approved.

## Contributor ladder

Maintainers are grown from contributors. Each rung is earned by doing the work,
not by asking for the title.

| Level | What you do | How you get there |
|---|---|---|
| **Contributor** | File issues, open PRs, join Discussions. | Your first merged PR. |
| **Regular contributor** | Repeated reviewed contributions; help triage and review others' PRs. | A track record of merged PRs and useful reviews. |
| **Reviewer** | Review PRs in an area and can approve them. | Nominated by a maintainer after sustained ownership of an area. |
| **Maintainer** | Merge rights, releases, security triage, roadmap. | Invited by existing maintainers; see [GOVERNANCE.md](GOVERNANCE.md). |

## First Contributions

New here? Look for issues labeled **`good first issue`** or **`help wanted`** — they are scoped to be self-contained and come with pointers. Comment on the issue to claim it, open a draft PR early for feedback, and ask in [Q&A](https://github.com/x1zzdev/Xazz/discussions/categories/q-a) when stuck — we would rather answer a question than have you guess.

### Good first issue rules

Maintainers curate `good first issue` items:

- The issue states a clear outcome and acceptance criteria.
- The fix is expected to touch one crate (or one IDE area) and add one test.
- A maintainer leaves a short "where to start" note in the issue.
- If the issue turns out larger than advertised, say so and split it rather than leaving a newcomer stuck.

## Releasing

Release cadence, the checklist, and the release-note template live in [docs/RELEASING.md](docs/RELEASING.md).

---

## Issue Guidelines

When filing a GitHub Issue, please include:

**For bug reports:**
- Xazz version (`xazz --version`)
- Operating system
- `.xzz` source that reproduces the issue (minimal reproduction preferred)
- Full error output

**For proposals in Discussions → Ideas:**
- What problem you are trying to solve
- What behavior you would expect
- Any relevant context

---

## Code Style

- Rust: follow `rustfmt` defaults. Run `cargo fmt` before committing.
- Other text files: follow the root [`.editorconfig`](.editorconfig) (UTF-8, LF, two-space indentation for YAML/TOML/JS/JSX; Rust remains four spaces).
- Commit messages: use conventional commit format (`feat:`, `fix:`, `docs:`, `chore:`, etc.).
- No Polars/Tokio imports in `xazz` (CLI) or `xazz-compiler` crates.

---

## Architecture Constraints

The following rules must be maintained:

1. `xazz` (CLI) dependencies must not include: `polars`, `polars-*`, `tokio`, `rayon`, `xazz-exec`, `xazz-runner`.
2. `xazz-exec` is only used by `xazz-runner` — never by the CLI directly.
3. `xazz-compiler` must not depend on Polars (parsing and codegen only).
4. New execution logic goes into `xazz-exec`.

See [docs/WORKSPACE.md](docs/WORKSPACE.md) for the full dependency graph.
