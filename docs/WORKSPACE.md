# Xazz — Workspace Architecture

## Binary Size Reduction Strategy

Rust 바이너리는 정적 링크됩니다.
바이너리 크기 감소는 오직 **의존성 그래프 격리**를 통해서만 달성됩니다.

---

## Final Workspace Structure

```
xazz-lang/
├── Cargo.toml              ← workspace + xazz CLI (루트 패키지)
├── src/                    ← xazz CLI (경량 — Polars/Tokio 없음)
│   ├── main.rs             ← run 명령어 → xazz-runner 서브프로세스 스폰
│   ├── cli.rs
│   ├── predict.rs
│   ├── project.rs
│   ├── schema.rs
│   ├── ux.rs
│   └── whoami.rs
│
├── xazz-core/              ← 공유 핵심 타입 (ZERO 무거운 의존성)
│   └── src/
│       ├── lib.rs
│       ├── ast.rs          ← AST 노드 (Expr, Stmt, PipelineOp, ...)
│       ├── token.rs        ← Token, Span
│       └── error.rs        ← CompileError, ErrorKind
│
├── xazz-compiler/          ← 컴파일러 (Polars 없음)
│   └── src/
│       ├── lib.rs
│       ├── ast.rs          ← xazz-core::ast 재노출
│       ├── token.rs        ← xazz-core::token 재노출
│       ├── error.rs        ← xazz-core::error 재노출
│       ├── lexer.rs
│       ├── parser.rs
│       ├── codegen.rs
│       ├── emitter.rs
│       └── main.rs         ← 컴파일 전용 (파싱+AST 출력)
│
├── xazz-exec/              ← 실행 엔진 (Polars 격리 크레이트)
│   └── src/
│       ├── lib.rs
│       └── runtime.rs      ← run_pipeline() — Polars LazyFrame 엔진
│
├── xazz-runner/            ← 실행 바이너리 (CLI가 서브프로세스로 스폰)
│   └── src/
│       └── main.rs         ← xazz-runner <file.xzz> [--verbose] [--output]
│
├── xazz-sde/               ← 합성 데이터 생성기 (독립 — CLI와 무관)
├── xazz-server/            ← REST API 서버 (독립 — CLI와 무관)
└── ...
```

---

## Dependency Graph

```
┌─────────────────────────────────────────────────────────────────┐
│                    xazz (CLI binary)                            │
│  clap + indicatif + colored + csv + anyhow + encoding_rs        │
│  ✅ NO Polars  ✅ NO Tokio  ✅ NO xazz-exec                     │
└────────────────┬────────────────────────────────────────────────┘
                 │ depends on
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                  xazz-compiler                                  │
│  Lexer + Parser + Codegen + Emitter                             │
│  ✅ NO Polars  ✅ NO Tokio                                       │
└────────────────┬────────────────────────────────────────────────┘
                 │ depends on
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                    xazz-core                                    │
│  AST + Token + Error  (serde 외 zero heavy deps)                │
└─────────────────────────────────────────────────────────────────┘

         [run 명령어: std::process::Command 서브프로세스 스폰]
xazz CLI ──spawn──► xazz-runner ──link──► xazz-exec ──link──► Polars
(통신: CLI args만)

┌─────────────────────────────────────────────────────────────────┐
│                  xazz-runner (binary)                           │
│  xazz-runner <file.xzz> [--verbose] [--output path.csv]        │
└────────────────┬────────────────────────────────────────────────┘
                 │ depends on
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                    xazz-exec                                    │
│  run_pipeline() — Polars LazyFrame 런타임                       │
│  ⚠️ Polars + encoding_rs (무거운 의존성 격리)                   │
└───────┬────────────────────┬───────────────────────────────────┘
        │                    │
        ▼                    ▼
   xazz-core          xazz-compiler


[독립 크레이트 — CLI 의존성 그래프 외부]

xazz-sde:    polars + rayon + xazz-compiler (독립 바이너리)
xazz-server: axum + tokio (독립 바이너리, xazz-compiler 미사용)
```

---

## Crate Responsibilities

| Crate | 역할 | 무거운 의존성 | CLI 링크 |
|---|---|---|---|
| `xazz` (CLI) | 인자 파싱, emit, import, check | 없음 | ✅ CLI 자신 |
| `xazz-core` | AST/Token/Error 공유 타입 + DL 타입 (v0.3) | 없음 (serde만) | ✅ 간접 |
| `xazz-compiler` | Lexer/Parser/Codegen/Emitter + DL 파싱 (v0.3) | 없음 | ✅ emit 명령어 |
| `xazz-exec` | Polars 실행 엔진 + Burn 플레이스홀더 (v0.3) | **Polars, encoding_rs** | ❌ 없음 |
| `xazz-runner` | 실행 바이너리 | xazz-exec 통해 간접 | ❌ 없음 |
| `xazz-server` | REST API + 보안/감사 엔드포인트 (v0.3) | axum, tokio, sha2 | ❌ 없음 |

---

## Execution Boundary (OPTION A — subprocess)

```
xazz run file.xzz
    │
    ├─ find_runner() → 같은 디렉토리의 xazz-runner.exe 또는 PATH
    │
    └─ std::process::Command::new("xazz-runner")
           .arg("file.xzz")
           .arg("--verbose")      // optional
           .arg("--output")       // optional
           .arg("result.csv")
           .status()
```

**통신 프로토콜:**  
- 입력: CLI arguments만 (JSON stdin 불필요)  
- 출력: xazz-runner의 stdout/stderr 그대로 전달  
- 종료 코드: xazz-runner의 exit code 전파

---

## Migration Summary

### Before (의존성 체인 — Polars가 CLI에 포함됨)
```
xazz CLI → xazz-compiler → polars (🚫 CLI 바이너리에 Polars 링크됨)
xazz CLI → tokio (🚫 비동기 런타임 링크됨)
```

### After (의존성 격리 — Polars가 CLI에서 제거됨)
```
xazz CLI → xazz-compiler → xazz-core → serde
xazz-runner → xazz-exec → polars (✅ 분리된 바이너리)
```

### Binary Size Impact (예상)
| Binary | Before | After | 차이 |
|---|---|---|---|
| `xazz` (CLI) | ~35MB+ (Polars 포함) | ~2-5MB | **~85% 감소** |
| `xazz-runner` | N/A | ~30MB+ | 실행 엔진 담당 |

---

## Build Commands

```bash
# 전체 워크스페이스 빌드
cargo build --release

# CLI 단독 빌드 (경량)
cargo build -p xazz --release

# 실행 엔진 단독 빌드 (Polars 포함)
cargo build -p xazz-runner --release

# 배포 시 두 바이너리를 같은 디렉토리에 배치
# xazz.exe + xazz-runner.exe
```

---

## Rules

1. `xazz` (CLI) 의 `[dependencies]` 에 절대 포함하면 안 되는 크레이트:
   - `polars`, `polars-*`
   - `tokio`
   - `rayon`
   - `xazz-exec`
   - `xazz-runner`

2. `xazz-exec` 는 CLI 의존성 그래프 외부에서만 사용한다.

3. `xazz-compiler` 는 Polars를 의존하지 않는다 (파싱/코드생성만).

4. 새로운 실행 로직은 반드시 `xazz-exec` 에 추가한다.
