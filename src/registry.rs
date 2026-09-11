// src/registry.rs — policy pack + stdlib registry (issue #68, E4)
//
// A tiny, offline-first registry: the three shipped domain policy packs and the
// standard-library modules are embedded in the CLI. `xazz registry list/show`
// inspect them, and `install` writes a copy into the current project — so a
// policy pack becomes `xazz.policy.json` (the auto-loaded policy path) and a
// stdlib module becomes a project-local `std/<name>.xzz` you can customize.
//
// A future remote registry can extend `ENTRIES` with a fetch step; the command
// surface and manifest shape stay the same.

use std::path::{Path, PathBuf};

/// What an entry installs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A Policy-as-Code JSON pack → `xazz.policy.json`
    PolicyPack,
    /// A `.xzz` standard-library module → `std/<name>.xzz`
    Stdlib,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::PolicyPack => "policy-pack",
            EntryKind::Stdlib => "stdlib",
        }
    }
}

/// One registry entry.
pub struct Entry {
    pub name: &'static str,
    pub kind: EntryKind,
    pub description: &'static str,
    /// Embedded file contents.
    pub source: &'static str,
}

/// The registry catalog — embedded, so it works offline.
pub const ENTRIES: &[Entry] = &[
    Entry {
        name: "healthcare",
        kind: EntryKind::PolicyPack,
        description: "Patient data (Medical Service Act §19) — direct identifiers, diagnosis, prescription",
        source: include_str!("../examples/security/healthcare_policy.json"),
    },
    Entry {
        name: "finance",
        kind: EntryKind::PolicyPack,
        description: "Credit data (Credit Information Act) — account/card numbers, credit score, transactions",
        source: include_str!("../examples/security/finance_policy.json"),
    },
    Entry {
        name: "public-sector",
        kind: EntryKind::PolicyPack,
        description: "Public data (Public Data Act) — RRN, civil complaints, benefit eligibility",
        source: include_str!("../examples/security/public_sector_policy.json"),
    },
    Entry {
        name: "common",
        kind: EntryKind::Stdlib,
        description: "Common schemas — TimeSeries, Measurement, AirQuality, Regression",
        source: include_str!("../xazz-stdlib/common.xzz"),
    },
    Entry {
        name: "math",
        kind: EntryKind::Stdlib,
        description: "Math/statistics helpers — Stats type, Linear and SmallMLP models",
        source: include_str!("../xazz-stdlib/math.xzz"),
    },
    Entry {
        name: "models",
        kind: EntryKind::Stdlib,
        description: "Reusable model architectures — LinearRegressor, MLPSmall, MLPMedium, MLPDeep",
        source: include_str!("../xazz-stdlib/models.xzz"),
    },
];

/// Looks up an entry by name.
pub fn find(name: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.name == name)
}

/// Default install destination for an entry.
fn default_dest(entry: &Entry) -> PathBuf {
    match entry.kind {
        EntryKind::PolicyPack => PathBuf::from("xazz.policy.json"),
        EntryKind::Stdlib => Path::new("std").join(format!("{}.xzz", entry.name)),
    }
}

/// `xazz registry list`
pub fn list() -> i32 {
    println!("Registry (embedded — offline)");
    println!("─────────────────────────────────────────────");
    for kind in [EntryKind::PolicyPack, EntryKind::Stdlib] {
        let label = match kind {
            EntryKind::PolicyPack => "POLICY PACKS",
            EntryKind::Stdlib => "STDLIB MODULES",
        };
        println!("\n{label}");
        for e in ENTRIES.iter().filter(|e| e.kind == kind) {
            println!("  {:<14} {}", e.name, e.description);
        }
    }
    println!("\nInstall with: xazz registry install <name>");
    0
}

/// `xazz registry show <name>`
pub fn show(name: &str) -> i32 {
    let Some(entry) = find(name) else {
        eprintln!("[xazz] registry: unknown entry '{name}'");
        eprintln!("        run `xazz registry list` to see available entries");
        return 1;
    };
    println!("name        : {}", entry.name);
    println!("kind        : {}", entry.kind.as_str());
    println!("description : {}", entry.description);
    println!("default out : {}", default_dest(entry).display());
    println!("─────────────────────────────────────────────");
    print!("{}", entry.source);
    if !entry.source.ends_with('\n') {
        println!();
    }
    0
}

/// `xazz registry install <name> [--out PATH] [--force]`
pub fn install(name: &str, out: Option<&Path>, force: bool) -> i32 {
    let Some(entry) = find(name) else {
        eprintln!("[xazz] registry: unknown entry '{name}'");
        eprintln!("        run `xazz registry list` to see available entries");
        return 1;
    };
    let dest = out
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| default_dest(entry));

    if dest.exists() && !force {
        eprintln!(
            "[xazz] registry: '{}' already exists (use --force to overwrite)",
            dest.display()
        );
        return 1;
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[xazz] registry: cannot create '{}': {e}", parent.display());
                return 1;
            }
        }
    }
    if let Err(e) = std::fs::write(&dest, entry.source) {
        eprintln!("[xazz] registry: cannot write '{}': {e}", dest.display());
        return 1;
    }

    println!(
        "✔ installed {} '{}' → {}",
        entry.kind.as_str(),
        entry.name,
        dest.display()
    );
    match entry.kind {
        EntryKind::PolicyPack => {
            println!("  The active policy is auto-loaded from ./xazz.policy.json.");
            println!("  Verify with: xazz policy <file.xzz>");
        }
        EntryKind::Stdlib => {
            println!(
                "  Import it as a project module: import \"{}\"",
                dest.display()
            );
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_unique_and_findable() {
        let mut names: Vec<&str> = ENTRIES.iter().map(|e| e.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "registry names must be unique");
        for e in ENTRIES {
            assert!(find(e.name).is_some(), "{} findable", e.name);
            assert!(!e.source.trim().is_empty(), "{} has content", e.name);
        }
    }

    #[test]
    fn policy_packs_parse_as_policy_json() {
        for e in ENTRIES.iter().filter(|e| e.kind == EntryKind::PolicyPack) {
            // The packs are real policy JSON — validate via the compiler's loader.
            xazz_compiler::policy::Policy::from_json_str(e.source)
                .unwrap_or_else(|err| panic!("{} is not a valid policy: {}", e.name, err.message));
        }
    }

    #[test]
    fn default_destinations() {
        let hc = find("healthcare").unwrap();
        assert_eq!(default_dest(hc), PathBuf::from("xazz.policy.json"));
        let models = find("models").unwrap();
        assert_eq!(default_dest(models), PathBuf::from("std/models.xzz"));
    }
}
