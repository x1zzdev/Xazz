// xazz-compiler/src/main.rs  (v0.18 → compile-only)
//
// Direct compiler execution entry point — parsing + AST output only.
//
// ⚠️  For runtime execution (Polars pipeline) use the xazz-runner binary.
//     This binary only performs the compile steps (Lexer → Parser → Codegen).
//
// Usage examples:
//   cargo run -p xazz-compiler -- examples/poc_script.xzz
//   cargo run -p xazz-compiler -- examples/poc_script.xzz --verbose

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input_path = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("examples/poc_script.xzz");

    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");

    // ── Read source file ─────────────────────────────────────────────────────
    let source = match std::fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[xazz-compiler] IO error: '{}' — {}", input_path, e);
            std::process::exit(1);
        }
    };

    eprintln!(
        "[xazz-compiler] input: {}  ({} bytes)",
        input_path,
        source.len()
    );

    // ── Lexer ────────────────────────────────────────────────────────────────
    let mut lexer = xazz_compiler::Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[xazz-compiler LEXER ERROR] {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("[xazz-compiler] Lexer done: {} tokens", tokens.len());

    if verbose {
        println!("\n⚡ STEP 1. Tokenized Stream");
        println!("{}", "─".repeat(60));
        for token in &tokens {
            println!(
                "  [{:>4}:{:<3}] {:?}",
                token.span.line, token.span.col, token.kind
            );
        }
    }

    // ── Parser ───────────────────────────────────────────────────────────────
    let mut parser = xazz_compiler::Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[xazz-compiler PARSER ERROR] {}", e);
            std::process::exit(1);
        }
    };
    eprintln!(
        "[xazz-compiler] Parser done: {} AST nodes",
        program.stmts.len()
    );

    if verbose {
        println!("\n⚡ STEP 2. Abstract Syntax Tree");
        println!("{}", "─".repeat(60));
        for (i, stmt) in program.stmts.iter().enumerate() {
            println!("  [{}] {:#?}", i, stmt);
        }
    }

    // ── Codegen ──────────────────────────────────────────────────────────────
    let codegen_output = xazz_compiler::Codegen::generate(&program);
    println!("\n⚡ STEP 3. Codegen Output");
    println!("{}", "─".repeat(60));
    println!("{}", codegen_output);

    eprintln!("[xazz-compiler] compile complete");
    eprintln!(
        "[xazz-compiler] ℹ️  to run it, use 'xazz run {}'.",
        input_path
    );
}
