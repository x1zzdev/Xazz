//! Opt-in diagnostic logging for the execution engine.
//!
//! Dependencies (cubecl/burn/Polars) emit diagnostics through the `log` facade,
//! but nothing installs a logger, so lines such as cubecl-wgpu's
//! `Using adapter {..}` are discarded. [`init`] installs a minimal stderr logger
//! when `XAZZ_LOG` (preferred) or `RUST_LOG` is set, making the selected GPU
//! adapter and other dependency diagnostics observable (issue #103, §6.1).
//!
//! The value is an `env_logger`-style filter: a bare level applies globally and
//! comma-separated `target=level` directives override it for matching module
//! paths (longest prefix wins):
//!
//! ```text
//! XAZZ_LOG=info
//! XAZZ_LOG=warn,cubecl_wgpu=debug
//! RUST_LOG=cubecl=debug
//! ```
//!
//! Logging is off unless one of the variables is set, so normal runs and the
//! default test suite stay quiet.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::io::Write;

/// Global level used when no target directive matches.
const DEFAULT_LEVEL: LevelFilter = LevelFilter::Off;

/// Parses a single level name (case-insensitive).
fn parse_level(raw: &str) -> Option<LevelFilter> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

/// An `env_logger`-style level filter: a global default plus per-target
/// directives matched by longest module-path prefix.
#[derive(Debug, Clone)]
struct Filter {
    default: LevelFilter,
    directives: Vec<(String, LevelFilter)>,
}

impl Filter {
    fn parse(spec: &str) -> Self {
        let mut filter = Self {
            default: DEFAULT_LEVEL,
            directives: Vec::new(),
        };
        for token in spec.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.split_once('=') {
                Some((target, level)) => {
                    let target = target.trim();
                    let Some(level) = parse_level(level) else {
                        continue;
                    };
                    if target.is_empty() {
                        filter.default = level;
                    } else {
                        filter.directives.push((target.to_string(), level));
                    }
                }
                None => {
                    if let Some(level) = parse_level(token) {
                        filter.default = level;
                    } else {
                        // `env_logger` treats a bare target as "trace for it".
                        filter
                            .directives
                            .push((token.to_string(), LevelFilter::Trace));
                    }
                }
            }
        }
        filter
    }

    /// Most permissive level the filter can emit, used to gate record creation.
    fn max_level(&self) -> LevelFilter {
        self.directives
            .iter()
            .map(|(_, level)| *level)
            .chain(std::iter::once(self.default))
            .max()
            .unwrap_or(LevelFilter::Off)
    }

    /// The effective level for `target`: the longest matching directive prefix,
    /// else the global default.
    fn level_for(&self, target: &str) -> LevelFilter {
        self.directives
            .iter()
            .filter(|(prefix, _)| target.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, level)| *level)
            .unwrap_or(self.default)
    }

    fn enabled(&self, target: &str, level: Level) -> bool {
        level <= self.level_for(target)
    }
}

/// Minimal stderr logger with no formatting beyond level and module target.
struct StderrLogger {
    filter: Filter,
}

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let _ = writeln!(
            std::io::stderr().lock(),
            "[xazz-exec] {} {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        let _ = std::io::stderr().lock().flush();
    }
}

/// Reads the filter spec from `XAZZ_LOG`, falling back to `RUST_LOG`.
fn spec_from_env() -> Option<String> {
    std::env::var("XAZZ_LOG")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok())
}

/// Installs the stderr logger once, if `XAZZ_LOG`/`RUST_LOG` is set.
///
/// Safe to call more than once: later calls are ignored. When neither variable
/// is set this is a no-op, so diagnostics are strictly opt-in.
pub fn init() {
    let Some(spec) = spec_from_env() else {
        return;
    };
    let filter = Filter::parse(&spec);
    let max = filter.max_level();
    if log::set_boxed_logger(Box::new(StderrLogger { filter })).is_ok() {
        log::set_max_level(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_level_sets_the_global_default() {
        let filter = Filter::parse("debug");
        assert_eq!(filter.default, LevelFilter::Debug);
        assert!(filter.directives.is_empty());
        assert!(filter.enabled("any::target", Level::Debug));
        assert!(!filter.enabled("any::target", Level::Trace));
    }

    #[test]
    fn directives_override_the_default() {
        let filter = Filter::parse("info,cubecl_wgpu=debug");
        assert_eq!(filter.default, LevelFilter::Info);
        assert_eq!(filter.level_for("cubecl_wgpu::x"), LevelFilter::Debug);
        assert_eq!(filter.level_for("polars::y"), LevelFilter::Info);
        assert!(filter.enabled("cubecl_wgpu::x", Level::Debug));
        assert!(!filter.enabled("polars::y", Level::Debug));
    }

    #[test]
    fn longest_prefix_wins() {
        let filter = Filter::parse("warn,cubecl=trace,cubecl_wgpu=debug");
        assert_eq!(filter.level_for("cubecl_wgpu::x"), LevelFilter::Debug);
        assert_eq!(filter.level_for("cubecl_core::y"), LevelFilter::Trace);
        assert_eq!(filter.level_for("other"), LevelFilter::Warn);
    }

    #[test]
    fn unknown_tokens_are_ignored_without_panicking() {
        let filter = Filter::parse("bogus,=,info");
        assert_eq!(filter.default, LevelFilter::Info);
        assert_eq!(filter.level_for("bogus"), LevelFilter::Trace);
    }

    #[test]
    fn empty_spec_disables_logging() {
        let filter = Filter::parse("  ");
        assert_eq!(filter.default, LevelFilter::Off);
        assert_eq!(filter.max_level(), LevelFilter::Off);
        assert!(!filter.enabled("x", Level::Error));
    }

    #[test]
    fn max_level_is_the_most_permissive() {
        let filter = Filter::parse("error,cubecl=debug");
        assert_eq!(filter.max_level(), LevelFilter::Debug);
    }
}
