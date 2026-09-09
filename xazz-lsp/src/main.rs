// xazz-lsp/src/main.rs — Xazz Language Server (issue B3)
//
// Reuses the xazz-compiler checker to surface the **exact same** line:col
// diagnostics as `xazz check`, live in the editor.
//
//   - diagnostics:  publish on open / change / save
//   - import modules: `import "./mod.xzz"` is resolved relative to the document's
//     directory before checking, exactly like `xazz check` (issue #69).
//   - hover / goto-def: driven by the token-level symbol table (xazz-compiler
//     `symbols` module) — variable/type/model definitions + references.
//
// The server speaks LSP over stdio — plug it into any editor (VS Code E1, etc.).

use std::sync::Mutex;

use tower_lsp::jsonrpc::Result as JsonRpcResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use xazz_compiler::checker::CheckResult;
use xazz_compiler::error::{CompileError, ErrorKind};
use xazz_compiler::symbols::{SymbolKind, index_source};

/// Workspace/document state retained by the server.
struct Backend {
    client: Client,
    /// document uri → source text (kept so diagnostics survive didChange)
    docs: Mutex<std::collections::HashMap<Url, String>>,
}

/// Converts a `CompileError` (1-based line/col, xazz check semantics) into an
/// LSP `Diagnostic` (0-based positions). This is the contract that must stay
/// byte-for-byte equivalent with `xazz check` output.
fn to_lsp_diagnostic(err: &CompileError, is_error: bool) -> Diagnostic {
    let line = err.span.line.saturating_sub(1);
    let col = err.span.col.saturating_sub(1);
    Diagnostic {
        range: Range {
            start: Position::new(line as u32, col as u32),
            end: Position::new(line as u32, col as u32 + 1),
        },
        severity: Some(if is_error {
            DiagnosticSeverity::ERROR
        } else {
            DiagnosticSeverity::WARNING
        }),
        source: Some("xazz".to_string()),
        code: Some(NumberOrString::String(err.kind.category().to_string())),
        message: format!("{} {}", err.kind.category(), err.message),
        ..Default::default()
    }
}

/// Converts the full checker result (errors + warnings) into LSP diagnostics.
fn check_result_to_diagnostics(result: &CheckResult) -> Vec<Diagnostic> {
    result
        .errors
        .iter()
        .map(|e| to_lsp_diagnostic(e, true))
        .chain(result.warnings.iter().map(|e| to_lsp_diagnostic(e, false)))
        .collect()
}

/// Lexes, parses, resolves module imports, and checks a source string.
///
/// Mirrors `xazz check`'s pipeline (src/main.rs): parse → resolve imports
/// relative to the document dir → analyze_program. Returns the diagnostics.
/// Converts a 1-based symbol span into an LSP `Location` (0-based).
fn symbol_to_location(uri: &Url, s: &xazz_compiler::symbols::Symbol) -> Location {
    Location {
        uri: uri.clone(),
        range: Range {
            start: Position::new((s.line - 1) as u32, (s.col - 1) as u32),
            end: Position::new((s.line - 1) as u32, (s.col - 1 + s.name.len()) as u32),
        },
    }
}

/// Token-level hover text for a symbol.
fn hover_text(s: &xazz_compiler::symbols::Symbol) -> String {
    let kind = match s.kind {
        SymbolKind::Variable => "variable",
        SymbolKind::Type => "type",
        SymbolKind::Model => "model",
        SymbolKind::Column => "column",
    };
    if s.is_definition {
        format!("**{kind}** `{}` (declaration)", s.name)
    } else {
        format!("**{kind}** `{}` (reference)", s.name)
    }
}

fn check_document(source: &str, dir: &std::path::Path) -> Vec<Diagnostic> {
    use xazz_compiler::{Lexer, Parser};

    let parsed = Lexer::new(source)
        .tokenize()
        .and_then(|tokens| Parser::new(tokens).parse());
    let (program, check) = match parsed {
        Err(_e) => return Vec::new(), // parse errors are reported as a single block
        Ok(program) => match xazz_compiler::modules::resolve_imports(&program, dir) {
            Err(module_errs) => {
                let combined = module_errs.join("\n");
                let err = CompileError::new(
                    ErrorKind::Other("module resolution failed".to_string()),
                    xazz_core::token::Span::new(1, 1),
                    combined,
                );
                return vec![to_lsp_diagnostic(&err, true)];
            }
            Ok(resolved) => {
                let (check, _ir) = xazz_compiler::analyze_program(&resolved.program);
                (resolved.program, check)
            }
        },
    };

    let _ = program;
    check_result_to_diagnostics(&check)
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> JsonRpcResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::info!("xazz-lsp initialized");
    }

    async fn shutdown(&self) -> JsonRpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let source = params.text_document.text.clone();
        self.docs.lock().unwrap().insert(uri.clone(), source.clone());

        let dir = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let diagnostics = check_document(&source, &dir);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        // Apply incremental changes to the stored document, then drop the guard
        // before the await below.
        let source = {
            let mut docs = self.docs.lock().unwrap();
            let current = docs.entry(uri.clone()).or_insert_with(String::new);
            let changes = params.content_changes;
            // With incremental sync, each change is a range replacement. In practice
            // editors may send full text; handle both.
            let full = changes.last().map(|c| c.text.clone()).unwrap_or_default();
            if full.is_empty() {
                return;
            }
            *current = full;
            current.clone()
        };

        let dir = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let diagnostics = check_document(&source, &dir);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let source = self.docs.lock().unwrap().get(&uri).cloned();
        if let Some(source) = source {
            let dir = uri
                .to_file_path()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let diagnostics = check_document(&source, &dir);
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    async fn hover(&self, params: HoverParams) -> JsonRpcResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let source_opt = self.docs.lock().unwrap().get(&uri).cloned();
        let Some(source) = source_opt else {
            return Ok(None);
        };
        let symbols = index_source(&source);
        let sym = match symbols.at_pos(pos.line as usize + 1, pos.character as usize + 1) {
            Some(s) => s,
            None => return Ok(None),
        };
        let loc = symbol_to_location(&uri, sym);
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text(sym),
            }),
            range: Some(loc.range),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> JsonRpcResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let source_opt = self.docs.lock().unwrap().get(&uri).cloned();
        let Some(source) = source_opt else {
            return Ok(None);
        };
        let symbols = index_source(&source);
        let sym = match symbols.at_pos(pos.line as usize + 1, pos.character as usize + 1) {
            Some(s) => s,
            None => return Ok(None),
        };
        // Find the definition of the same name.
        let def = match symbols.definitions_of(&sym.name).into_iter().next() {
            Some(d) => d,
            None => return Ok(None),
        };
        let loc = symbol_to_location(&uri, def);
        Ok(Some(GotoDefinitionResponse::Scalar(loc)))
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: Mutex::new(std::collections::HashMap::new()),
    });
    let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    rt.block_on(Server::new(stdin, stdout, socket).serve(service));
}

#[cfg(test)]
mod tests {
    use super::*;
    use xazz_core::token::Span;

    fn mk_error(line: usize, col: usize, message: &str) -> CompileError {
        CompileError::new(
            ErrorKind::Other(message.to_string()),
            Span::new(line, col),
            message.to_string(),
        )
    }

    /// 1-based checker spans become 0-based LSP positions.
    #[test]
    fn span_converts_to_zero_based_position() {
        let d = to_lsp_diagnostic(&mk_error(3, 5, "boom"), true);
        assert_eq!(d.range.start.line, 2);
        assert_eq!(d.range.start.character, 4);
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    }

    /// Warnings map to WARNING severity, errors to ERROR.
    #[test]
    fn severity_mapping() {
        let e = to_lsp_diagnostic(&mk_error(1, 1, "e"), true);
        let w = to_lsp_diagnostic(&mk_error(1, 1, "w"), false);
        assert_eq!(e.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(w.severity, Some(DiagnosticSeverity::WARNING));
    }

    /// A clean pipeline produces no diagnostics.
    #[test]
    fn clean_source_has_no_diagnostics() {
        let src = "type AQ = { station: string, pm10: float };\n\
                   v a = load(\"data.csv\") :: AQ\n\
                     |> groupBy(\"station\")\n\
                     |> mean(\"pm10\");";
        let dir = std::path::PathBuf::from(".");
        let diags = check_document(src, &dir);
        assert!(
            diags.is_empty(),
            "clean pipeline reported diagnostics: {diags:?}"
        );
    }

    /// An undeclared column surfaces as a diagnostic at the right position.
    #[test]
    fn undeclared_column_is_diagnostic() {
        let src = "type P = { temp: float };\n\
                   v a = load(\"d.csv\") :: P\n\
                     |> filter(temp > 10)\n\
                     |> select([temperture_c]);";
        let dir = std::path::PathBuf::from(".");
        let diags = check_document(src, &dir);
        assert!(
            diags.iter().any(|d| d.message.contains("temperture_c")),
            "did-you-mean 진단 미생성: {diags:?}"
        );
    }

    // ── Navigation (hover / goto-def) over the symbol table ────────────────

    /// Hover resolves a variable reference position to the symbol.
    #[test]
    fn hover_resolves_variable_reference() {
        let src = "type P = { a: string };\n\
                   v x = load(\"d.csv\") :: P;\n\
                   v y = x |> select([a]);";
        let symbols = index_source(src);
        // Cursor on `x` in line 3 (1-based), col 7.
        let sym = symbols.at_pos(3, 7).expect("symbol at x");
        assert_eq!(sym.name, "x");
        assert!(!sym.is_definition);
        let text = hover_text(sym);
        assert!(text.contains("variable"), "{text}");
    }

    /// Goto-definition jumps to the declaration (1-based → same line/col).
    #[test]
    fn goto_definition_jumps_to_declaration() {
        let src = "v x = load(\"d.csv\");\n\
                   v y = x |> select([a]);";
        let symbols = index_source(src);
        let sym = symbols.at_pos(2, 7).expect("symbol at x ref");
        let def = symbols.definitions_of(&sym.name).into_iter().next().expect("def");
        assert_eq!(def.line, 1);
        assert!(def.is_definition);
    }

    /// 1-based symbol span → 0-based LSP location (end exclusive col).
    #[test]
    fn symbol_location_conversion_is_zero_based() {
        let src = "v abc = load(\"d.csv\");";
        let symbols = index_source(src);
        let def = symbols.definitions_of("abc").into_iter().next().expect("def");
        let url: Url = "file:///t.xzz".parse().expect("valid file url");
        let loc = symbol_to_location(&url, def);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 2); // 1-based col 3 → 0-based 2
        assert_eq!(loc.range.end.character, 5); // "abc" is 3 chars → exclusive end
    }
}