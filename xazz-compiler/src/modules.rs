// xazz-compiler/src/modules.rs — module resolution for `import "path.xzz"` (issue #69)
//
// Xazz modules are plain `.xzz` files whose declarations (type / model / v pipelines)
// are merged into the importing program at the import site. Resolution is:
//
//   - Relative paths resolve against the importing file's directory.
//   - Cycles are detected via the canonical-path import stack (fail-closed error).
//   - Each module is inlined exactly once (import-once): repeated or diamond
//     imports of the same module do not duplicate declarations, while the
//     existing checker still validates user-written duplicates across the
//     merged AST.
//   - Module source texts are returned alongside the merged program so the
//     policy guardrail can scan each imported file's literals too.
//
// The compiler crate stays Polars-free; this module only uses std::fs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::{Program, Stmt};
use crate::{Lexer, Parser};

/// Embedded standard-library modules (`import "std/<name>"`), issue #56 (B2).
///
/// The `.xzz` sources live in the workspace `xazz-stdlib/` directory (single
/// source of truth) and are embedded at compile time so `import "std/..."`
/// resolves everywhere with no filesystem configuration.
const STDLIB_MODULES: &[(&str, &str)] = &[
    ("common", include_str!("../../xazz-stdlib/common.xzz")),
    ("math", include_str!("../../xazz-stdlib/math.xzz")),
    ("models", include_str!("../../xazz-stdlib/models.xzz")),
];

/// Looks up an embedded stdlib module by name (no `.xzz` extension).
fn stdlib_source(name: &str) -> Option<&'static str> {
    let name = name.trim_end_matches(".xzz");
    STDLIB_MODULES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, src)| *src)
}

/// A program whose imports have been expanded, plus each imported module's
/// (path, source text) so callers can run the policy literal scan per file.
#[derive(Debug, Clone, Default)]
pub struct ResolvedProgram {
    /// Merged AST — import statements replaced by the module's statements.
    pub program: Program,
    /// (module_path, source_text) for every imported module, in resolution order.
    pub modules: Vec<(PathBuf, String)>,
}

/// Resolves all `import "path.xzz"` statements reachable from `program`.
///
/// `source_dir` is the directory of the importing file (used to resolve
/// relative module paths). On failure (missing file, parse error, cycle)
/// returns Err with a user-facing message.
pub fn resolve_imports(
    program: &Program,
    source_dir: &Path,
) -> Result<ResolvedProgram, Vec<String>> {
    let mut loader = ModuleLoader {
        stack: Vec::new(),
        loaded: HashSet::new(),
        modules: Vec::new(),
        errors: Vec::new(),
    };
    let merged = loader.load_program(program, source_dir, false);
    if loader.errors.is_empty() {
        Ok(ResolvedProgram {
            program: merged,
            modules: loader.modules,
        })
    } else {
        Err(loader.errors)
    }
}

/// Normalizes a canonicalized path for cross-platform-stable comparison.
///
/// On Windows `Path::canonicalize` returns verbatim paths (`\\?\C:\...` or
/// `\\?\UNC\server\share`). The extended-length prefix is a filesystem
/// implementation detail and can make otherwise-identical paths compare unequal
/// (e.g. when mixed with a non-verbatim `source_dir`), which shows up as a
/// false-positive cycle / failed dedup. Strip it so the import stack and the
/// loaded-set always compare the same normalized form.
fn normalize_canonical(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

struct ModuleLoader {
    /// Canonical paths of the current import chain (for cycle detection).
    stack: Vec<PathBuf>,
    /// Canonical paths already resolved (avoid re-reading shared modules).
    loaded: HashSet<PathBuf>,
    /// (canonical path, source text) in resolution order.
    modules: Vec<(PathBuf, String)>,
    errors: Vec<String>,
}

impl ModuleLoader {
    /// Recursively expands imports in a program, returning a merged Program.
    ///
    /// `source_dir` is the directory of the file that owns `program`, so each
    /// relative import resolves against its *importing* file — including nested
    /// modules in subdirectories.
    ///
    /// When `embedded` is true the program belongs to an embedded stdlib
    /// module: a relative import then names a sibling std module and resolves
    /// in the embedded namespace, never against the process CWD.
    fn load_program(&mut self, program: &Program, source_dir: &Path, embedded: bool) -> Program {
        let mut out = Program::new();
        for stmt in &program.stmts {
            match stmt {
                Stmt::Import { path } => {
                    // Embedded stdlib: `import "std/<name>"` (issue #56, B2).
                    if let Some(rest) = path.strip_prefix("std/") {
                        let name = rest.trim_end_matches(".xzz");
                        match stdlib_source(name) {
                            Some(text) => self.load_embedded(name, text, &mut out),
                            None => self.errors.push(format!(
                                "unknown stdlib module: 'std/{name}' (available: common, math, models)"
                            )),
                        }
                        continue;
                    }
                    // Inside an embedded module a relative import is a sibling
                    // std module (`import "math"`), resolved in the embedded
                    // namespace so `.`/CWD is never consulted.
                    if embedded {
                        let name = path.trim_end_matches(".xzz");
                        match stdlib_source(name) {
                            Some(text) => self.load_embedded(name, text, &mut out),
                            None => self.errors.push(format!(
                                "unknown stdlib module: 'std/{name}' (available: common, math, models)"
                            )),
                        }
                        continue;
                    }
                    let file = self.resolve_module_path(path, source_dir);
                    match file {
                        Some(canonical) => self.load_module(&canonical, &mut out),
                        None => self
                            .errors
                            .push(format!("import \"{path}\": cannot resolve path")),
                    }
                }
                other => out.stmts.push(other.clone()),
            }
        }
        out
    }

    /// Resolves an import path against the importing file's directory,
    /// canonicalizes it, and reports a helpful error if the file is missing.
    fn resolve_module_path(&mut self, path: &str, source_dir: &Path) -> Option<PathBuf> {
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            source_dir.join(path)
        };
        if !candidate.exists() {
            self.errors.push(format!(
                "import module file not found: '{}' (resolved from '{}')",
                path,
                source_dir.display()
            ));
            return None;
        }
        candidate.canonicalize().ok().map(normalize_canonical)
    }

    fn load_module(&mut self, canonical: &PathBuf, out: &mut Program) {
        // Import-once: a module already inlined elsewhere in the merged AST is
        // not inlined again. Cycles are still caught below via the import stack
        // (a module is marked loaded only after its body is expanded).
        if self.loaded.contains(canonical) {
            return;
        }

        // ── cycle detection ────────────────────────────────────────────────
        if self.stack.contains(canonical) {
            let mut chain: Vec<String> =
                self.stack.iter().map(|p| p.display().to_string()).collect();
            chain.push(canonical.display().to_string());
            self.errors
                .push(format!("cyclic import detected: {}", chain.join(" → ")));
            return;
        }

        // ── read + parse ────────────────────────────────────────────────────
        let text = match std::fs::read_to_string(canonical) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(format!(
                    "failed to read module '{}': {}",
                    canonical.display(),
                    e
                ));
                return;
            }
        };
        let tokens = match Lexer::new(&text).tokenize() {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(format!(
                    "module '{}' lexer error: {}",
                    canonical.display(),
                    e
                ));
                return;
            }
        };
        let parsed = match Parser::new(tokens).parse() {
            Ok(p) => p,
            Err(e) => {
                self.errors.push(format!(
                    "module '{}' parser error: {}",
                    canonical.display(),
                    e
                ));
                return;
            }
        };

        // ── push context, expand nested imports, inline ─────────────────────
        self.stack.push(canonical.clone());
        // Nested imports resolve against *this* module's directory, not the
        // root program's (fixes subdirectory-relative imports).
        let module_dir = canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        // Import-once: expand nested imports, then mark this module loaded and
        // inline its declarations. A later import of the same module returns
        // early, so shared (diamond) modules are inlined exactly once.
        let nested = self.load_program(&parsed, &module_dir, false);
        self.stack.pop();

        self.loaded.insert(canonical.clone());
        self.modules.push((canonical.clone(), text));
        for stmt in nested.stmts {
            out.stmts.push(stmt);
        }
    }

    /// Loads an embedded stdlib module (issue #56, B2). Uses a synthetic
    /// `std/<name>` key for cycle detection and dedup, mirroring `load_module`.
    fn load_embedded(&mut self, name: &str, text: &'static str, out: &mut Program) {
        let key = PathBuf::from(format!("std/{name}"));

        // Import-once, mirroring `load_module`: a std module already inlined is
        // skipped. Cycles are still caught via the stack below.
        if self.loaded.contains(&key) {
            return;
        }

        if self.stack.contains(&key) {
            self.errors
                .push(format!("cyclic stdlib import detected: std/{name}"));
            return;
        }

        let tokens = match Lexer::new(text).tokenize() {
            Ok(t) => t,
            Err(e) => {
                self.errors
                    .push(format!("stdlib module 'std/{name}' lexer error: {e}"));
                return;
            }
        };
        let parsed = match Parser::new(tokens).parse() {
            Ok(p) => p,
            Err(e) => {
                self.errors
                    .push(format!("stdlib module 'std/{name}' parser error: {e}"));
                return;
            }
        };

        self.stack.push(key.clone());
        // Embedded modules have no filesystem directory: relative imports are
        // resolved in the embedded stdlib namespace (`embedded = true`), so the
        // `source_dir` value is inert.
        let nested = self.load_program(&parsed, Path::new("."), true);
        self.stack.pop();

        self.loaded.insert(key.clone());
        self.modules.push((key, text.to_string()));
        for stmt in nested.stmts {
            out.stmts.push(stmt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Monotonic per-process discriminator. The wall clock can be too coarse on
    /// some platforms (notably Windows) for two parallel tests to get distinct
    /// nanosecond stamps, which previously made them share a temp directory and
    /// race on cleanup. The atomic counter guarantees a unique suffix.
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        let unique = format!(
            "xazz_mod_test_{}_{}_{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        base.join(unique)
    }

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src).tokenize().expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    #[test]
    fn module_import_inlines_declarations() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("prep.xzz"),
            "type Prep = { a: int, b: Option<float> };\n\
             v cleaned = load(\"prep.csv\") :: Prep |> dropNull(\"b\");\n",
        )
        .unwrap();

        let main = parse("import \"prep.xzz\";\nv out = cleaned |> sum(\"a\");");
        let resolved = resolve_imports(&main, &dir).expect("module resolve");
        assert_eq!(resolved.modules.len(), 1, "모듈 1개 로드");
        assert_eq!(
            resolved.program.stmts.len(),
            3,
            "import 1 + 모듈 2 = 3 문장"
        );
        assert!(
            matches!(resolved.program.stmts[0], Stmt::TypeDecl { .. }),
            "첫 문장이 모듈의 type 선언이어야 함"
        );
        assert!(
            matches!(resolved.program.stmts[1], Stmt::VarDecl { .. }),
            "두번째가 모듈의 v 파이프라인이어야 함"
        );
        // 모듈에 정의된 변수 `cleaned` 가 main 의 참조에서 해석되는지 (체커 통과 여부)
        let (check, _ir) = crate::analyze_program(&resolved.program);
        assert!(
            check.errors.is_empty(),
            "모듈 병합 체커 오류 없어야: {:?}",
            check.errors
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// A nested module resolves its own relative imports against its own
    /// directory, not the root program's (issue #69 follow-up).
    #[test]
    fn nested_import_resolves_against_importing_file_dir() {
        let dir = temp_dir();
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("b.xzz"), "type B = { b: int };\n").unwrap();
        fs::write(
            sub.join("a.xzz"),
            "import \"b.xzz\";\ntype A = { a: int };\n",
        )
        .unwrap();
        // A decoy `b.xzz` at the root proves resolution uses sub/, not the root.
        fs::write(dir.join("b.xzz"), "type WRONG = { wrong: int };\n").unwrap();

        let main = parse("import \"sub/a.xzz\";");
        let resolved = resolve_imports(&main, &dir).expect("nested module resolve");

        let names: Vec<&str> = resolved
            .program
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::TypeDecl { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            names.contains(&"A"),
            "sub/a.xzz 가 해석되어야 함: {names:?}"
        );
        assert!(
            names.contains(&"B"),
            "sub/b.xzz 가 해석되어야 함: {names:?}"
        );
        assert!(
            !names.contains(&"WRONG"),
            "루트 b.xzz 를 잘못 선택하면 안 됨: {names:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// A shared module imported via a diamond (b→d, c→d) is inlined exactly
    /// once, so the merged program has no duplicate declarations and the
    /// checker passes (import-once semantics).
    #[test]
    fn diamond_import_inlines_shared_module_once() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("d.xzz"), "type D = { d: int };\n").unwrap();
        fs::write(
            dir.join("b.xzz"),
            "import \"d.xzz\";\ntype B = { b: int };\n",
        )
        .unwrap();
        fs::write(
            dir.join("c.xzz"),
            "import \"d.xzz\";\ntype C = { c: int };\n",
        )
        .unwrap();

        let main = parse("import \"b.xzz\";\nimport \"c.xzz\";");
        let resolved = resolve_imports(&main, &dir).expect("diamond resolve");

        let d_count = resolved
            .program
            .stmts
            .iter()
            .filter(|s| matches!(s, Stmt::TypeDecl { name, .. } if name == "D"))
            .count();
        assert_eq!(d_count, 1, "공유 모듈 D 는 정확히 1회만 인라인되어야 함");
        assert_eq!(
            resolved
                .modules
                .iter()
                .filter(|(p, _)| p.ends_with("d.xzz"))
                .count(),
            1,
            "공유 모듈 소스도 1회만 기록"
        );
        let (check, _ir) = crate::analyze_program(&resolved.program);
        assert!(
            check.errors.is_empty(),
            "import-once 병합은 체커 오류가 없어야: {:?}",
            check.errors
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// Importing the same file twice inlines it once.
    #[test]
    fn repeated_import_is_deduplicated() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("d.xzz"), "type D = { d: int };\n").unwrap();

        let main = parse("import \"d.xzz\";\nimport \"d.xzz\";");
        let resolved = resolve_imports(&main, &dir).expect("repeat resolve");
        let d_count = resolved
            .program
            .stmts
            .iter()
            .filter(|s| matches!(s, Stmt::TypeDecl { name, .. } if name == "D"))
            .count();
        assert_eq!(d_count, 1, "동일 파일 반복 import 는 1회 인라인");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cyclic_import_is_detected() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.xzz"),
            "import \"b.xzz\";\ntype A = { a: int };\n",
        )
        .unwrap();
        fs::write(
            dir.join("b.xzz"),
            "import \"a.xzz\";\ntype B = { b: int };\n",
        )
        .unwrap();

        let main = parse("import \"a.xzz\";");
        let err = resolve_imports(&main, &dir).expect_err("사이클은 오류");
        assert!(
            err.iter().any(|e| e.contains("cyclic import")),
            "사이클 진단: {:?}",
            err
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_module_is_an_error() {
        let dir = temp_dir();
        let main = parse("import \"nope.xzz\";");
        let err = resolve_imports(&main, &dir).expect_err("없는 파일은 오류");
        assert!(
            err.iter().any(|e| e.contains("not found")),
            "누락 진단: {:?}",
            err
        );
    }

    /// Embedded stdlib modules resolve without any filesystem config (B2).
    #[test]
    fn stdlib_import_resolves_embedded() {
        let main = parse("import \"std/models\";");
        let resolved = resolve_imports(&main, &std::env::temp_dir()).expect("stdlib resolve");
        assert!(
            resolved.program.stmts.iter().any(|s| matches!(
                s,
                Stmt::ModelDecl { name, .. } if name == "MLPMedium"
            )),
            "std/models 의 model 들이 인라인되어야 함"
        );
        assert!(
            resolved
                .modules
                .iter()
                .any(|(p, _)| p.to_string_lossy().contains("std/models")),
            "모듈 출처가 std/models 로 기록되어야 함"
        );
    }

    /// `std/common` + `std/math` also resolve, and an unknown name errors.
    #[test]
    fn stdlib_common_and_math_resolve() {
        let main = parse("import \"std/common\";\nimport \"std/math\";");
        let resolved = resolve_imports(&main, &std::env::temp_dir()).expect("stdlib resolve");
        assert!(resolved.program.stmts.iter().any(|s| matches!(
            s,
            Stmt::TypeDecl { name, .. } if name == "TimeSeries"
        )));
        assert!(resolved.program.stmts.iter().any(|s| matches!(
            s,
            Stmt::ModelDecl { name, .. } if name == "SmallMLP"
        )));
    }

    #[test]
    fn embedded_stdlib_imports_pass_the_checker() {
        for name in ["common", "math", "models"] {
            let main = parse(&format!("import \"std/{name}\";"));
            let resolved = resolve_imports(&main, &std::env::temp_dir()).expect("stdlib resolve");
            let (check, _) = crate::analyze_program(&resolved.program);
            assert!(
                check.errors.is_empty(),
                "std/{name} checker errors: {:?}",
                check.errors
            );
        }
    }

    #[test]
    fn unknown_stdlib_module_errors() {
        let main = parse("import \"std/nope\";");
        let err = resolve_imports(&main, &std::env::temp_dir()).expect_err("unknown stdlib");
        assert!(
            err.iter().any(|e| e.contains("unknown stdlib")),
            "미지원 stdlib 진단: {:?}",
            err
        );
    }

    /// An embedded stdlib module resolves a sibling std module by relative name
    /// inside the embedded namespace, never against the process CWD (issue #69
    /// follow-up: inject a stdlib source base for embedded modules).
    #[test]
    fn embedded_module_relative_import_resolves_sibling() {
        let mut loader = ModuleLoader {
            stack: Vec::new(),
            loaded: HashSet::new(),
            modules: Vec::new(),
            errors: Vec::new(),
        };
        let mut out = Program::new();
        loader.load_embedded("common", "import \"math\";", &mut out);
        assert!(
            loader.errors.is_empty(),
            "임베디드 상대 import 는 오류가 없어야: {:?}",
            loader.errors
        );
        assert!(
            out.stmts.iter().any(|s| matches!(
                s,
                Stmt::ModelDecl { name, .. } if name == "SmallMLP"
            )),
            "std/math 의 model 이 인라인되어야 함"
        );
    }

    /// A std module imported twice (directly or via a sibling) is inlined once.
    #[test]
    fn embedded_repeated_import_is_deduplicated() {
        let mut loader = ModuleLoader {
            stack: Vec::new(),
            loaded: HashSet::new(),
            modules: Vec::new(),
            errors: Vec::new(),
        };
        let mut out = Program::new();
        loader.load_embedded("common", "import \"math\";\nimport \"math\";", &mut out);
        assert!(
            loader.errors.is_empty(),
            "임베디드 반복 import 는 오류가 없어야: {:?}",
            loader.errors
        );
        let count = out
            .stmts
            .iter()
            .filter(|s| matches!(s, Stmt::ModelDecl { name, .. } if name == "SmallMLP"))
            .count();
        assert_eq!(count, 1, "std/math 는 정확히 1회만 인라인되어야 함");
    }

    /// A relative import inside an embedded module that is not a known std
    /// module errors instead of probing the filesystem.
    #[test]
    fn embedded_module_relative_import_unknown_errors() {
        let mut loader = ModuleLoader {
            stack: Vec::new(),
            loaded: HashSet::new(),
            modules: Vec::new(),
            errors: Vec::new(),
        };
        let mut out = Program::new();
        loader.load_embedded("common", "import \"nope\";", &mut out);
        assert!(
            loader
                .errors
                .iter()
                .any(|e| e.contains("unknown stdlib") && e.contains("nope")),
            "미지원 임베디드 상대 import 진단: {:?}",
            loader.errors
        );
    }
}
