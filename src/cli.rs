use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Shared help for every `--actor` flag. The audit actor is sent to the server as
/// `X-Xazz-Actor`, but a server configured with `XAZZ_ADMIN_ACTORS` pins the actor
/// to the credential and ignores that header, so `--actor` only takes effect for
/// an unbound admin token (`XAZZ_ADMIN_TOKEN`).
const ACTOR_HELP: &str = "Audit actor for an admin-delegated change (sent as X-Xazz-Actor); ignored when the server pins the actor via XAZZ_ADMIN_ACTORS";

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
    /// Example: xazz run demo/preprocess_chart.xzz
    ///
    /// Example: xazz run demo/preprocess_chart.xzz --output result.csv
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
        /// Example: xazz run demo/preprocess_chart.xzz --json
        #[arg(long)]
        json: bool,

        /// Enable the typed IR optimization pass (e.g. filter reordering)
        ///
        /// Example: xazz run demo/preprocess_chart.xzz --opt
        #[arg(long)]
        opt: bool,
    },

    /// Run static semantic analysis (type checker) on .xzz code before execution
    ///
    /// Detects undeclared variables/models/schemas, columns not in a schema,
    /// and type mismatches before execution.
    ///
    /// Example: xazz check demo/preprocess_chart.xzz
    ///
    /// Example: xazz check demo/preprocess_chart.xzz --json
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
    ///
    /// Example: xazz policy examples/security/patient_unsafe.xzz --fix
    ///
    /// Example: xazz policy examples/security/patient_unsafe.xzz --fix --out safe.xzz --json
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
    /// Example: xazz emit rust demo/preprocess_chart.xzz --out output.rs
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
    /// Example: xazz import visual-ide/data/seoul_air_quality.csv
    /// Example: xazz import data/semicolon.csv --delimiter ';' --no-header
    Import {
        /// Path to the CSV file to import
        file: String,

        /// Field delimiter for CSV parsing (single ASCII character; default comma)
        #[arg(long, value_name = "CHAR")]
        delimiter: Option<char>,

        /// Treat the first row as data instead of a header (columns become column_1..N)
        #[arg(long)]
        no_header: bool,
    },

    /// Run fine-tuning data sanitization checks (PII / duplicates / bias) (issue #72, F3)
    ///
    /// Example: xazz sanitize data/train.csv
    /// Example: xazz sanitize data/train.csv --json
    Sanitize {
        /// Path to the data file to check (CSV/Parquet/Arrow)
        file: String,

        /// Print the structured JSON report (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Browse and install policy packs / stdlib modules (issue #68, E4)
    ///
    /// Example: xazz registry list
    ///
    /// Example: xazz registry show healthcare
    ///
    /// Example: xazz registry install healthcare --out xazz.policy.json
    ///
    /// Example: xazz registry install models --out std/models.xzz
    Registry {
        #[command(subcommand)]
        action: RegistryAction,
    },

    /// Read a tenant's policy-pack change history / retention window from a
    /// running server (issue C2)
    ///
    /// `history` defaults to the policy-pack change history; `ttl` reports the
    /// effective history retention window and `ttl-history` its change log. All
    /// three are read-only and tenant-scoped (sent as X-Xazz-Tenant).
    ///
    /// Example: xazz policy-status history --tenant acme --token $TOKEN
    ///
    /// Example: xazz policy-status ttl --tenant acme
    ///
    /// Example: xazz policy-status history --cursor 42 --limit 10 --json
    PolicyStatus {
        /// Which read-only view to fetch
        #[arg(value_enum, default_value = "history")]
        view: PolicyView,

        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        /// Id cursor from a previous page (`next_cursor`); history views only
        #[arg(long)]
        cursor: Option<i64>,

        /// Page size for the history views
        #[arg(long)]
        limit: Option<usize>,

        /// Print the server's JSON body verbatim instead of a human summary
        #[arg(long)]
        json: bool,
    },

    /// Set or clear a tenant's policy-history retention window on a running
    /// server (issue C2)
    ///
    /// `set` stores a per-tenant override with `PUT /security/policy/history/ttl`
    /// (`--ttl-secs 0` means "keep forever"); `clear` removes the override with
    /// `DELETE /security/policy/history/ttl` so the tenant falls back to the
    /// global default. Both are tenant-scoped (sent as X-Xazz-Tenant) and accept
    /// an admin `--actor` for a delegated change.
    ///
    /// Example: xazz policy-ttl set --ttl-secs 86400 --tenant acme --token $TOKEN
    ///
    /// Example: xazz policy-ttl clear --tenant acme --token $TOKEN
    PolicyTtl {
        #[command(subcommand)]
        action: PolicyTtlAction,
    },

    /// Inspect or change a tenant's DP budget on a running server (issue C2)
    ///
    /// `xazz dp window set` stores a per-tenant window override
    /// (`PUT /dp/budget/window`, `--window-secs 0` means "cumulative, no window");
    /// `clear` removes it (`DELETE /dp/budget/window`) so the tenant falls back to
    /// the global `XAZZ_TENANT_DP_WINDOW_SECS` default; `history` reads the
    /// append-only change log (`GET /dp/budget/window/history`). `xazz dp budget`
    /// reads the tenant's current spend/remaining envelope (`GET /dp/budget`).
    /// `xazz dp reset` clears the tenant's ledger (`POST /dp/budget/reset`, admin
    /// `--actor` for a delegated reset) and `xazz dp reset-history` reads the
    /// append-only reset log (`GET /dp/budget/history`). All are tenant-scoped
    /// (sent as X-Xazz-Tenant).
    ///
    /// Example: xazz dp window set --window-secs 86400 --tenant acme --token $TOKEN
    ///
    /// Example: xazz dp window clear --tenant acme --token $TOKEN
    ///
    /// Example: xazz dp window history --tenant acme --cursor 42 --limit 10 --json
    ///
    /// Example: xazz dp budget --tenant acme --token $TOKEN
    ///
    /// Example: xazz dp reset --tenant acme --token $TOKEN
    ///
    /// Example: xazz dp reset-history --tenant acme --json
    Dp {
        #[command(subcommand)]
        action: DpAction,
    },

    /// Analyze the xazz user profile and confirm the identity
    ///
    /// Example: xazz whoami
    #[command(hide = true)]
    Whoami,
}

/// Mutating policy-history retention-window actions for `xazz policy-ttl` (issue C2).
#[derive(Subcommand, Debug)]
pub enum PolicyTtlAction {
    /// Store a per-tenant retention-window override (`--ttl-secs 0` = keep forever)
    ///
    /// Example: xazz policy-ttl set --ttl-secs 86400 --tenant acme --token $TOKEN
    Set {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Retention window in seconds; 0 disables time-based expiry for the tenant
        #[arg(long, value_name = "SECS")]
        ttl_secs: u64,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,
    },

    /// Remove the tenant's override so it falls back to the global default
    ///
    /// Example: xazz policy-ttl clear --tenant acme --token $TOKEN
    Clear {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,
    },
}

/// Subcommands for `xazz dp` (issue C2).
#[derive(Subcommand, Debug)]
pub enum DpAction {
    /// Manage the tenant's DP budget window override
    Window {
        #[command(subcommand)]
        action: DpWindowAction,
    },

    /// Read the tenant's current DP budget spend, remaining envelope, and window
    ///
    /// Example: xazz dp budget --tenant acme --token $TOKEN
    /// Example: xazz dp budget --tenant acme --json
    Budget {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        /// Print the server's JSON body verbatim instead of a human summary
        #[arg(long)]
        json: bool,
    },

    /// Clear the tenant's accumulated DP spend and re-anchor its budget window
    ///
    /// Example: xazz dp reset --tenant acme --token $TOKEN
    /// Example: xazz dp reset --tenant acme --actor admin --token $TOKEN
    Reset {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,

        /// Print the server's JSON body verbatim instead of a human summary
        #[arg(long)]
        json: bool,
    },

    /// Read the tenant's append-only DP budget reset history
    ///
    /// Example: xazz dp reset-history --tenant acme --json
    /// Example: xazz dp reset-history --tenant acme --cursor 42 --limit 10 --json
    ResetHistory {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        /// Return rows strictly older than this id (from a previous next_cursor)
        #[arg(long)]
        cursor: Option<i64>,

        /// Maximum number of reset records to return (server-clamped)
        #[arg(long)]
        limit: Option<usize>,

        /// Print the server's JSON body verbatim instead of a human summary
        #[arg(long)]
        json: bool,
    },
}

/// DP budget window actions for `xazz dp window` (issue C2).
#[derive(Subcommand, Debug)]
pub enum DpWindowAction {
    /// Store a per-tenant window override (`--window-secs 0` = cumulative)
    ///
    /// Example: xazz dp window set --window-secs 86400 --tenant acme --token $TOKEN
    Set {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Sliding window in seconds; 0 disables the window (cumulative budget)
        #[arg(long, value_name = "SECS")]
        window_secs: u64,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,
    },

    /// Remove the tenant's override so it falls back to the global default
    ///
    /// Example: xazz dp window clear --tenant acme --token $TOKEN
    Clear {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,
    },

    /// Read the tenant's append-only window override change history
    ///
    /// Example: xazz dp window history --tenant acme --cursor 42 --limit 10 --json
    History {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        /// Id cursor from a previous page (`next_cursor`)
        #[arg(long)]
        cursor: Option<i64>,

        /// Page size
        #[arg(long)]
        limit: Option<usize>,

        /// Print the server's JSON body verbatim instead of a human summary
        #[arg(long)]
        json: bool,
    },
}

/// Read-only policy views for `xazz policy-status` (issue C2).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyView {
    /// Policy-pack change history (`GET /security/policy/history`)
    History,
    /// Effective policy-history retention window (`GET /security/policy/history/ttl`)
    Ttl,
    /// Retention-window change history (`GET /security/policy/history/ttl/history`)
    TtlHistory,
}

/// Registry subcommands (issue #68, E4).
#[derive(Subcommand, Debug)]
pub enum RegistryAction {
    /// List available policy packs and stdlib modules
    List,

    /// Show details for a pack/module by name
    Show {
        /// Registry entry name (e.g. healthcare, models)
        name: String,
    },

    /// Install a pack/module into the current project
    ///
    /// Policy packs default to `xazz.policy.json`; stdlib modules default to
    /// `std/<name>.xzz`.
    Install {
        /// Registry entry name
        name: String,

        /// Destination path (default depends on the entry kind)
        #[arg(long)]
        out: Option<PathBuf>,

        /// Overwrite an existing destination file
        #[arg(long)]
        force: bool,
    },

    /// Deploy a policy pack to a tenant through a running server
    ///
    /// Deploy an embedded registry pack by name, a local policy JSON file with
    /// `--file` (`-` reads stdin), or `./xazz.policy.json` when neither is given.
    /// Writes the pack to `PUT /security/policy` in the target tenant namespace
    /// (issue C2). Stdlib modules cannot be deployed.
    ///
    /// Example: xazz registry deploy healthcare --tenant acme --token $TOKEN
    /// Example: xazz registry deploy --file xazz.policy.json --tenant acme --token $TOKEN
    /// Example: xazz registry deploy finance --server http://127.0.0.1:8005 \
    ///          --tenant acme --token $ADMIN --actor ops
    Deploy {
        /// Registry policy-pack name (e.g. healthcare, finance); omit when using --file
        name: Option<String>,

        /// Deploy a local policy JSON file instead of an embedded pack; `-` reads
        /// stdin, and the default is ./xazz.policy.json
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,

        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,
    },

    /// Remove a tenant's deployed policy pack through a running server
    ///
    /// Deletes the pack stored under the target tenant namespace via
    /// `DELETE /security/policy` (issue C2); the tenant then falls back to the
    /// global/builtin policy. Tenant-scoped, and an admin actor can be recorded
    /// with `--actor`.
    ///
    /// Example: xazz registry undeploy --tenant acme --token $TOKEN
    /// Example: xazz registry undeploy --server http://127.0.0.1:8005 \
    ///          --tenant acme --token $ADMIN --actor ops
    Undeploy {
        /// Xazz server base URL
        #[arg(long, default_value = "http://127.0.0.1:8005")]
        server: String,

        /// Target tenant namespace (sent as X-Xazz-Tenant)
        #[arg(long)]
        tenant: String,

        /// Bearer token — defaults to XAZZ_ADMIN_TOKEN, then XAZZ_SERVER_TOKEN
        #[arg(long)]
        token: Option<String>,

        #[arg(long, help = ACTOR_HELP)]
        actor: Option<String>,
    },
}
