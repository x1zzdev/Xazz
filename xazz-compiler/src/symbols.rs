// xazz-compiler/src/symbols.rs — symbol index for editor navigation (issue #75, B3)
//
// Builds a lightweight symbol table from a token stream: where each variable /
// type / model is **defined** (with its 1-based line:col span) and where it is
// **referenced**. This is what drives LSP hover, go-to-definition, and rename
// without heavyweight analysis — the checker's Typed IR is column-focused, so
// token-level symbol extraction is the most direct source.
//
// The indexer is deliberately independent of semantic analysis: a symbol is a
// name + a role, not a type. Correctness of *types* stays with the checker.

use crate::token::{Token, TokenKind};
use xazz_core::i18n::{is_korean, tr};
use xazz_core::token::Span;

/// What a symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A data variable (`v name = ...`)
    Variable,
    /// A schema declaration (`type Name = { ... }`)
    Type,
    /// A model declaration (`model Name { ... }`)
    Model,
    /// A column name inside a schema, or a referenced column literal
    Column,
}

impl SymbolKind {
    /// Human-readable label for hover text (localized).
    pub fn label(self) -> &'static str {
        use xazz_core::i18n::is_korean;
        if is_korean() {
            match self {
                SymbolKind::Variable => "변수",
                SymbolKind::Type => "타입",
                SymbolKind::Model => "모델",
                SymbolKind::Column => "컬럼",
            }
        } else {
            match self {
                SymbolKind::Variable => "variable",
                SymbolKind::Type => "type",
                SymbolKind::Model => "model",
                SymbolKind::Column => "column",
            }
        }
    }
}

/// One symbol occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The symbol name (as written)
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based line
    pub line: usize,
    /// 1-based col
    pub col: usize,
    /// True when this occurrence declares the symbol, false when it references it.
    pub is_definition: bool,
}

/// Result of indexing a source file.
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
}

impl SymbolTable {
    /// Definitions of a given name.
    pub fn definitions_of(&self, name: &str) -> Vec<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| s.name == name && s.is_definition)
            .collect()
    }

    /// All references (including definitions) to a name.
    pub fn references_of(&self, name: &str) -> Vec<&Symbol> {
        self.symbols.iter().filter(|s| s.name == name).collect()
    }

    /// The symbol at a (1-based) line/col — for hover.
    pub fn at_pos(&self, line: usize, col: usize) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|s| s.line == line && s.col <= col && col <= s.col + s.name.len())
    }
}

/// Indexes a token stream into a symbol table.
///
/// Declarations recognized from the xazz grammar:
///   `v <name>` / `mut v <name>`  → Variable
///   `type <name>`                → Type
///   `model <name>`               → Model
///
/// References: every subsequent `Ident` matching a known declared name is
/// recorded as a (non-definition) reference, except the declaration site itself.
pub fn index_tokens(tokens: &[Token]) -> SymbolTable {
    let mut table = SymbolTable::default();
    let mut declared: Vec<(String, SymbolKind)> = Vec::new();
    let mut declared_spans: Vec<(String, SymbolKind, Span)> = Vec::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        match &tok.kind {
            TokenKind::Type => {
                // type Name = { ... }
                if let Some(next) = tokens.get(i + 1) {
                    if let TokenKind::Ident(name) = &next.kind {
                        // report the declaration span
                        table.symbols.push(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Type,
                            line: next.span.line,
                            col: next.span.col,
                            is_definition: true,
                        });
                        declared.push((name.clone(), SymbolKind::Type));
                        declared_spans.push((name.clone(), SymbolKind::Type, next.span.clone()));
                        i += 2;
                        continue;
                    }
                }
            }
            TokenKind::Model => {
                if let Some(next) = tokens.get(i + 1) {
                    if let TokenKind::Ident(name) = &next.kind {
                        table.symbols.push(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Model,
                            line: next.span.line,
                            col: next.span.col,
                            is_definition: true,
                        });
                        declared.push((name.clone(), SymbolKind::Model));
                        declared_spans.push((name.clone(), SymbolKind::Model, next.span.clone()));
                        i += 2;
                        continue;
                    }
                }
            }
            TokenKind::V => {
                // v Name = ...
                if let Some(next) = tokens.get(i + 1) {
                    if let TokenKind::Ident(name) = &next.kind {
                        table.symbols.push(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Variable,
                            line: next.span.line,
                            col: next.span.col,
                            is_definition: true,
                        });
                        declared.push((name.clone(), SymbolKind::Variable));
                        declared_spans.push((
                            name.clone(),
                            SymbolKind::Variable,
                            next.span.clone(),
                        ));
                        i += 2;
                        continue;
                    }
                }
            }
            TokenKind::Mut => {
                // mut v Name = ...  — the name follows `mut v`.
                // Advance past v if present, then read the identifier.
                let mut j = i + 1;
                if matches!(tokens.get(j).map(|t| &t.kind), Some(TokenKind::V)) {
                    j += 1;
                }
                if let Some(next) = tokens.get(j) {
                    if let TokenKind::Ident(name) = &next.kind {
                        table.symbols.push(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Variable,
                            line: next.span.line,
                            col: next.span.col,
                            is_definition: true,
                        });
                        declared.push((name.clone(), SymbolKind::Variable));
                        declared_spans.push((
                            name.clone(),
                            SymbolKind::Variable,
                            next.span.clone(),
                        ));
                        i = j + 1;
                        continue;
                    }
                }
            }
            TokenKind::Ident(name) => {
                // A reference to a previously declared symbol.
                if declared.iter().any(|(d, _)| d == name) {
                    // Avoid re-flagging the declaration span itself.
                    let is_def_span = declared_spans.iter().any(|(d, _, sp)| {
                        d == name && sp.line == tok.span.line && sp.col == tok.span.col
                    });
                    if !is_def_span {
                        table.symbols.push(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Variable, // kind refined below if needed
                            line: tok.span.line,
                            col: tok.span.col,
                            is_definition: false,
                        });
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Kind for references: match a previously declared kind where unambiguous.
    for sym in table.symbols.iter_mut() {
        if !sym.is_definition {
            if let Some((_, kind)) = declared.iter().rev().find(|(d, _)| d == &sym.name) {
                sym.kind = *kind;
            }
        }
    }

    table
}

/// Builds a symbol table from a source string (lexes internally).
pub fn index_source(source: &str) -> SymbolTable {
    match crate::Lexer::new(source).tokenize() {
        Ok(tokens) => index_tokens(&tokens),
        Err(_) => SymbolTable::default(),
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_type_variable_model_definitions() {
        let src = "type P = { a: string };\n\
                   v x = load(\"d.csv\") :: P;\n\
                   model M { Dense(4) -> Dense(1) }\n\
                   v t = x |> train(M, target: \"a\");";
        let t = index_source(src);

        let defs: Vec<&Symbol> = t.symbols.iter().filter(|s| s.is_definition).collect();
        // P (type), x (var), M (model), t (var)
        assert_eq!(defs.len(), 4, "{:#?}", defs);
        assert_eq!(defs[0].name, "P");
        assert_eq!(defs[0].kind, SymbolKind::Type);
        assert_eq!(defs[0].line, 1);
        assert_eq!(defs[1].name, "x");
        assert_eq!(defs[1].kind, SymbolKind::Variable);
        assert_eq!(defs[2].name, "M");
        assert_eq!(defs[2].kind, SymbolKind::Model);
        assert_eq!(defs[3].name, "t");
    }

    #[test]
    fn records_references() {
        let src = "v x = load(\"d.csv\");\n\
                   v y = x |> filter(a > 1);";
        let t = index_source(src);
        // x is referenced in the second statement.
        let refs: Vec<Symbol> = t.references_of("x").into_iter().cloned().collect();
        let def = refs.iter().find(|s| s.is_definition).expect("def");
        assert_eq!(def.line, 1);
        let uses: Vec<&Symbol> = refs.iter().filter(|s: &&Symbol| !s.is_definition).collect();
        assert!(!uses.is_empty(), "no references found: {:#?}", refs);
        assert_eq!(uses[0].line, 2);
    }

    #[test]
    fn hover_position_resolves() {
        let src = "v x = load(\"d.csv\");\n\
                   v y = x |> select([a]);";
        let t = index_source(src);
        // Cursor on `x` in line 2 (col 7).
        let sym = t.at_pos(2, 7).expect("symbol at pos");
        assert_eq!(sym.name, "x");
    }

    #[test]
    fn mut_variable_declaration_indexed() {
        let src = "mut v data = load(\"d.csv\");\nv c2 = data;";
        let t = index_source(src);
        let defs: Vec<&Symbol> = t.symbols.iter().filter(|s| s.is_definition).collect();
        assert_eq!(defs.len(), 2, "{:#?}", defs);
        assert_eq!(defs[0].name, "data");
        assert!(t.references_of("data").iter().any(|s| !s.is_definition));
    }
}
