// xazz-compiler/src/modules.rs — module resolution for `import "path.xzz"` (issue #69)
//
// Xazz modules are plain `.xzz` files whose declarations (type / model / v pipelines)
// are merged into the importing program at the import site. Resolution is:
//
//   - Relative paths resolve against the importing file's directory.
//   - Cycles are detected via the canonical-path import stack (fail-closed error).
//   - Imported declarations are inlined in order, so the existing checker's
//     duplicate-detection and reference resolution run unchanged on the merged AST.
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
        source_dir: source_dir.to_path_buf(),
        stack: Vec::new(),
        loaded: HashSet::new(),
        modules: Vec::new(),
        errors: Vec::new(),
    };
    let merged = loader.load_program(program);
    if loader.errors.is_empty() {
        Ok(ResolvedProgram {
            program: merged,
            modules: loader.modules,
        })
    } else {
        Err(loader.errors)
    }
}

struct ModuleLoader {
    source_dir: PathBuf,
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
    fn load_program(&mut self, program: &Program) -> Program {
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
                    let file = self.resolve_module_path(path);
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

    /// Resolves an import path against the source directory, canonicalizes it,
    /// and reports a helpful error if the file is missing.
    fn resolve_module_path(&mut self, path: &str) -> Option<PathBuf> {
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.source_dir.join(path)
        };
        if !candidate.exists() {
            self.errors.push(format!(
                "import module file not found: '{}' (resolved from '{}')",
                path,
                self.source_dir.display()
            ));
            return None;
        }
        candidate.canonicalize().ok()
    }

    fn load_module(&mut self, canonical: &PathBuf, out: &mut Program) {
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
        // Load each module exactly once (dedupe shared modules), but inline
        // into this importer every time — duplicate names surface as checker
        // errors, which is the intended fail-closed behavior.
        let nested = self.load_program(&parsed);
        self.stack.pop();

        if self.loaded.insert(canonical.clone()) {
            self.modules.push((canonical.clone(), text));
        }
        for stmt in nested.stmts {
            out.stmts.push(stmt);
        }
    }

    /// Loads an embedded stdlib module (issue #56, B2). Uses a synthetic
    /// `std/<name>` key for cycle detection and dedup, mirroring `load_module`.
    fn load_embedded(&mut self, name: &str, text: &'static str, out: &mut Program) {
        let key = PathBuf::from(format!("std/{name}"));

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
        let nested = self.load_program(&parsed);
        self.stack.pop();

        if self.loaded.insert(key.clone()) {
            self.modules.push((key, text.to_string()));
        }
        for stmt in nested.stmts {
            out.stmts.push(stmt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        let unique = format!(
            "xazz_mod_test_{}_{}",
            std::process::id(),
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
        let resolved =
            resolve_imports(&main, &std::env::temp_dir()).expect("stdlib resolve");
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
        let resolved =
            resolve_imports(&main, &std::env::temp_dir()).expect("stdlib resolve");
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
    fn unknown_stdlib_module_errors() {
        let main = parse("import \"std/nope\";");
        let err = resolve_imports(&main, &std::env::temp_dir()).expect_err("unknown stdlib");
        assert!(
            err.iter().any(|e| e.contains("unknown stdlib")),
            "미지원 stdlib 진단: {:?}",
            err
        );
    }
}
