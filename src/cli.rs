use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Xazz unified CLI — compiler · static analysis · Rust emit · synthetic data generator
#[derive(Parser, Debug)]
#[command(
    name = "xazz",
    version,
    author,
    about = "Xazz unified toolchain: run, check, emit, and generate synthetic data"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run xazz data analysis code
    ///
    /// Example: xazz run examples/poc_script.xzz
    /// Example: xazz run examples/pipeline.xzz --output result.csv
    Run {
        /// Path to the .xzz source file to run
        file: PathBuf,

        /// Enable release mode optimizations
        #[arg(short, long)]
        release: bool,

        /// Verbose mode: print the lexer token stream and AST
        #[arg(short, long)]
        verbose: bool,

        /// Save the execution result to a CSV file
        ///
        /// Example: --output result.csv
        #[arg(long)]
        output: Option<PathBuf>,

        /// Print the structured JSON execution result (machine-readable)
        ///
        /// Example: xazz run examples/poc_script.xzz --json
        #[arg(long)]
        json: bool,

        /// Enable the typed IR optimization pass (e.g. filter reordering)
        ///
        /// Example: xazz run examples/pipeline.xzz --opt
        #[arg(long)]
        opt: bool,
    },

    /// Run static semantic analysis (type checker) on .xzz code before execution
    ///
    /// Detects undeclared variables/models/schemas, columns not in a schema,
    /// and type mismatches before execution.
    ///
    /// Example: xazz check examples/poc_script.xzz
    /// Example: xazz check examples/poc_script.xzz --json
    Check {
        /// Path to the .xzz source file to analyze
        file: PathBuf,

        /// Print the structured JSON diagnostics result (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Check .xzz code with Policy-as-Code security guardrails (issue #2)
    ///
    /// Detects direct PII exposure, re-identification risk, and hardcoded secrets
    /// before execution; with --fix, also proposes a safe alternative.
    ///
    /// Example: xazz policy examples/security/patient_unsafe.xzz
    /// Example: xazz policy examples/security/patient_unsafe.xzz --fix
    /// Example: xazz policy pipeline.xzz --fix --out safe.xzz --json
    Policy {
        /// Path to the .xzz source file to check
        file: PathBuf,

        /// Print the structured JSON report (machine-readable)
        #[arg(long)]
        json: bool,

        /// Also propose a safe alternative with violations auto-remediated
        #[arg(long)]
        fix: bool,

        /// Path to save the remediated code (used with --fix)
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Convert a .xzz script to another language/format and output it
    ///
    /// Example: xazz emit rust examples/poc_script.xzz --out output.rs
    Emit {
        /// Output format (currently supported: rust)
        format: String,

        /// Path to the .xzz source file to convert
        file: PathBuf,

        /// Output file path (prints to stdout if not specified)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Automatically generate synthetic training data pairs
    ///
    /// Example: xazz sde --rows 5000 --output data/pairs/pairs.jsonl
    Sde {
        /// Number of data rows to generate
        #[arg(long, default_value_t = 10000)]
        rows: usize,

        /// Output file path
        #[arg(long, default_value = "data/pairs/pairs.jsonl")]
        output: PathBuf,
    },

    /// Create a new Xazz project
    ///
    /// Example: xazz new my-project
    New {
        /// Name of the project to create
        name: String,
    },

    /// Read a CSV file and add the type definition and load statement to main.xzz
    ///
    /// Example: xazz import data/seoul_air.csv
    Import {
        /// Path to the CSV file to import
        file: String,
    },

    /// Analyze the xazz user profile and confirm the identity
    ///
    /// Example: xazz whoami
    #[command(hide = true)]
    Whoami,
}
