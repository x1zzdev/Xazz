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
cargo test
```

---

## First Contributions

Not sure where to start? Look for issues labeled **`good first issue`** or **`help wanted`** — they're a great place to begin. We're happy to answer questions and help you get your first PR merged. Don't hesitate to open a draft PR early for feedback.

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
