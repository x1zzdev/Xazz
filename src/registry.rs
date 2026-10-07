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

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::http;

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
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("[xazz] registry: cannot create '{}': {e}", parent.display());
        return 1;
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

/// The server endpoint a policy pack is deployed to (issue C2).
///
/// Pure so the URL join is testable without a server. `server` is the base URL;
/// a trailing slash is optional.
pub fn policy_endpoint(server: &str) -> String {
    format!("{}/security/policy", server.trim().trim_end_matches('/'))
}

/// Token fallback for `xazz registry deploy` — the admin token wins over the
/// single-server token, matching the server's authentication precedence.
pub(crate) fn env_token() -> Option<String> {
    ["XAZZ_ADMIN_TOKEN", "XAZZ_SERVER_TOKEN"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Default policy file consulted by `registry deploy` when neither a registry
/// name nor `--file` is given. This is the same path the rest of the CLI treats
/// as the active policy.
pub const DEFAULT_POLICY_FILE: &str = "xazz.policy.json";

/// Validates a policy pack JSON string, mapping the compiler error to a message.
fn validate_policy_source(source: &str, label: &str) -> Result<(), String> {
    xazz_compiler::policy::Policy::from_json_str(source)
        .map(|_| ())
        .map_err(|err| format!("'{label}' is not a valid policy: {}", err.message))
}

/// Resolves the policy pack source for [`deploy`].
///
/// `name` selects an embedded registry pack; `file` reads a local JSON file, or
/// `-` for stdin; when both are absent the default [`DEFAULT_POLICY_FILE`] is
/// used if it exists. A local/stdin source is validated with the compiler's
/// policy loader before any network call. `default_path` is parameterised so the
/// fallback is testable without changing the process working directory.
fn resolve_deploy_source(
    name: Option<&str>,
    file: Option<&Path>,
    default_path: &Path,
    stdin: &mut dyn Read,
) -> Result<(String, String), String> {
    match (name, file) {
        (Some(_), Some(_)) => Err("pass either a registry name or --file, not both".to_string()),
        (Some(name), None) => {
            let entry = find(name).ok_or_else(|| {
                format!(
                    "unknown entry '{name}'\n        run `xazz registry list` to see available entries"
                )
            })?;
            if entry.kind != EntryKind::PolicyPack {
                return Err(format!(
                    "'{}' is a {} and cannot be deployed as a tenant policy",
                    entry.name,
                    entry.kind.as_str()
                ));
            }
            Ok((entry.name.to_string(), entry.source.to_string()))
        }
        (None, Some(path)) if path == Path::new("-") => {
            let mut source = String::new();
            stdin
                .read_to_string(&mut source)
                .map_err(|e| format!("cannot read policy from stdin: {e}"))?;
            validate_policy_source(&source, "<stdin>")?;
            Ok(("<stdin>".to_string(), source))
        }
        (None, Some(path)) => {
            let source = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
            validate_policy_source(&source, &path.display().to_string())?;
            Ok((path.display().to_string(), source))
        }
        (None, None) => {
            if !default_path.exists() {
                return Err("provide a registry name or --file <path>".to_string());
            }
            let source = std::fs::read_to_string(default_path)
                .map_err(|e| format!("cannot read '{}': {e}", default_path.display()))?;
            validate_policy_source(&source, &default_path.display().to_string())?;
            Ok((default_path.display().to_string(), source))
        }
    }
}

/// `xazz registry deploy [name] [--file PATH] --tenant T [--server URL] [--token TOKEN] [--actor A]`
///
/// Deploys a policy pack to a tenant namespace through the server's
/// `PUT /security/policy` endpoint (issue C2). The pack source is an embedded
/// registry entry (`name`), a local policy JSON file (`--file`, or `-` for
/// stdin), or the default [`DEFAULT_POLICY_FILE`] when neither is given. Stdlib
/// modules are project-local files and are rejected before any network call.
pub fn deploy(
    name: Option<&str>,
    file: Option<&Path>,
    server: &str,
    tenant: &str,
    token: Option<&str>,
    actor: Option<&str>,
) -> i32 {
    let mut stdin = std::io::stdin();
    let (label, source) =
        match resolve_deploy_source(name, file, Path::new(DEFAULT_POLICY_FILE), &mut stdin) {
            Ok(resolved) => resolved,
            Err(message) => {
                eprintln!("[xazz] registry: {message}");
                return 1;
            }
        };

    let tenant = tenant.trim();
    if tenant.is_empty() {
        eprintln!("[xazz] registry: --tenant must not be empty");
        return 1;
    }
    if server.trim().is_empty() {
        eprintln!("[xazz] registry: --server must not be empty");
        return 1;
    }

    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(env_token);
    let Some(token) = token else {
        eprintln!(
            "[xazz] registry: no token — pass --token or set XAZZ_ADMIN_TOKEN / XAZZ_SERVER_TOKEN"
        );
        return 1;
    };

    let endpoint = policy_endpoint(server);
    let mut headers: Vec<(&str, String)> = vec![
        ("Authorization", format!("Bearer {token}")),
        ("X-Xazz-Tenant", tenant.to_string()),
    ];
    if let Some(actor) = actor.map(str::trim).filter(|value| !value.is_empty()) {
        headers.push(("X-Xazz-Actor", actor.to_string()));
    }

    match http::put_json(&endpoint, &headers, &source) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            println!("✔ deployed policy-pack '{label}' → tenant '{tenant}'");
            println!("  server: {endpoint}");
            0
        }
        Ok(resp) => {
            eprintln!(
                "[xazz] registry: server returned HTTP {} for tenant '{}'",
                resp.status, tenant
            );
            let body = resp.body.trim();
            if !body.is_empty() {
                eprintln!("        {body}");
            }
            1
        }
        Err(e) => {
            eprintln!("[xazz] registry: deploy failed — {e}");
            1
        }
    }
}

/// `xazz registry undeploy --tenant T [--server URL] [--token TOKEN] [--actor A]`
///
/// Removes the policy pack stored under a tenant namespace through the server's
/// `DELETE /security/policy` endpoint (issue C2). After removal the tenant falls
/// back to the global/builtin policy. Validation mirrors [`deploy`], so an empty
/// tenant/server or a missing token fails locally before any network call.
pub fn undeploy(server: &str, tenant: &str, token: Option<&str>, actor: Option<&str>) -> i32 {
    let tenant = tenant.trim();
    if tenant.is_empty() {
        eprintln!("[xazz] registry: --tenant must not be empty");
        return 1;
    }
    if server.trim().is_empty() {
        eprintln!("[xazz] registry: --server must not be empty");
        return 1;
    }

    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(env_token);
    let Some(token) = token else {
        eprintln!(
            "[xazz] registry: no token — pass --token or set XAZZ_ADMIN_TOKEN / XAZZ_SERVER_TOKEN"
        );
        return 1;
    };

    let endpoint = policy_endpoint(server);
    let mut headers: Vec<(&str, String)> = vec![
        ("Authorization", format!("Bearer {token}")),
        ("X-Xazz-Tenant", tenant.to_string()),
    ];
    if let Some(actor) = actor.map(str::trim).filter(|value| !value.is_empty()) {
        headers.push(("X-Xazz-Actor", actor.to_string()));
    }

    match http::delete_json(&endpoint, &headers) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            println!("✔ undeployed policy-pack from tenant '{tenant}'");
            println!("  server: {endpoint}");
            0
        }
        Ok(resp) => {
            eprintln!(
                "[xazz] registry: server returned HTTP {} for tenant '{}'",
                resp.status, tenant
            );
            let body = resp.body.trim();
            if !body.is_empty() {
                eprintln!("        {body}");
            }
            1
        }
        Err(e) => {
            eprintln!("[xazz] registry: undeploy failed — {e}");
            1
        }
    }
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

    #[test]
    fn policy_endpoint_joins_base_and_trims_slash() {
        assert_eq!(
            policy_endpoint("http://127.0.0.1:8005"),
            "http://127.0.0.1:8005/security/policy"
        );
        assert_eq!(
            policy_endpoint(" http://127.0.0.1:8005/ "),
            "http://127.0.0.1:8005/security/policy"
        );
        assert_eq!(
            policy_endpoint("http://host/api/"),
            "http://host/api/security/policy"
        );
    }

    #[test]
    fn deploy_rejects_stdlib_entry_before_any_network_call() {
        let code = deploy(
            Some("models"),
            None,
            "http://127.0.0.1:1",
            "acme",
            Some("secret"),
            None,
        );
        assert_eq!(code, 1, "stdlib modules are not deployable");
    }

    #[test]
    fn deploy_requires_name_or_file() {
        let code = deploy(
            None,
            None,
            "http://127.0.0.1:1",
            "acme",
            Some("secret"),
            None,
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn deploy_rejects_name_and_file_together() {
        let code = deploy(
            Some("healthcare"),
            Some(Path::new("xazz.policy.json")),
            "http://127.0.0.1:1",
            "acme",
            Some("secret"),
            None,
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn deploy_reports_unreadable_file() {
        let code = deploy(
            None,
            Some(Path::new("does-not-exist-xazz.policy.json")),
            "http://127.0.0.1:1",
            "acme",
            Some("secret"),
            None,
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn resolve_deploy_source_reads_stdin() {
        let pack = find("healthcare").unwrap().source;
        let mut stdin: &[u8] = pack.as_bytes();
        let missing = Path::new("does-not-exist-default.json");
        let (label, source) =
            resolve_deploy_source(None, Some(Path::new("-")), missing, &mut stdin).unwrap();
        assert_eq!(label, "<stdin>");
        assert_eq!(source, pack);
    }

    #[test]
    fn resolve_deploy_source_rejects_invalid_stdin() {
        let mut stdin: &[u8] = b"{ not a policy }";
        let missing = Path::new("does-not-exist-default.json");
        let error = resolve_deploy_source(None, Some(Path::new("-")), missing, &mut stdin)
            .expect_err("invalid stdin policy must be rejected");
        assert!(error.contains("not a valid policy"), "{error}");
    }

    #[test]
    fn resolve_deploy_source_falls_back_to_default_file() {
        let pack = find("healthcare").unwrap().source;
        let path = write_temp_policy(pack);
        let mut stdin: &[u8] = b"";
        let (label, source) = resolve_deploy_source(None, None, &path, &mut stdin).unwrap();
        assert_eq!(source, pack);
        assert_eq!(label, path.display().to_string());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolve_deploy_source_errors_without_name_file_or_default() {
        let missing = std::env::temp_dir().join("xazz-no-such-default-policy.json");
        let _ = std::fs::remove_file(&missing);
        let mut stdin: &[u8] = b"";
        let error = resolve_deploy_source(None, None, &missing, &mut stdin)
            .expect_err("no source must be an error");
        assert!(
            error.contains("provide a registry name or --file"),
            "{error}"
        );
    }

    #[test]
    fn deploy_rejects_invalid_local_policy_before_any_network_call() {
        let path = write_temp_policy("{ not a policy }");
        let code = deploy(
            None,
            Some(&path),
            "http://127.0.0.1:1",
            "acme",
            Some("secret"),
            None,
        );
        assert_eq!(code, 1, "invalid policy JSON must fail before the network");
        let _ = std::fs::remove_file(path);
    }

    /// Writes `contents` to a unique temp `.json` path (process id + nanos).
    fn write_temp_policy(contents: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("xazz-registry-{}-{nonce}.json", std::process::id()));
        std::fs::write(&path, contents).expect("write temp policy file");
        path
    }

    /// Minimal one-shot HTTP server: captures the raw request and replies with the
    /// given status line/body. Lets `deploy` be verified end-to-end without a real
    /// xazz-server.
    fn spawn_policy_server(
        status_line: &str,
        response_body: &str,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake server");
        let addr = listener.local_addr().expect("local addr");
        let status_line = status_line.to_string();
        let response_body = response_body.to_string();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 1024];
            let header_end = loop {
                let n = stream.read(&mut chunk).expect("read request");
                if n == 0 {
                    break buf.len();
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let content_length = head
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            while buf.len() < header_end + content_length {
                let n = stream.read(&mut chunk).expect("read body");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            tx.send(String::from_utf8_lossy(&buf).into_owned())
                .expect("send captured request");

            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush response");
        });

        (format!("http://{addr}"), rx)
    }

    #[test]
    fn deploy_puts_pack_to_tenant_endpoint() {
        let (server, rx) = spawn_policy_server("HTTP/1.1 200 OK", r#"{"tenant":"acme"}"#);
        let code = deploy(
            Some("healthcare"),
            None,
            &server,
            "acme",
            Some("secret"),
            Some("admin"),
        );
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("PUT /security/policy HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.contains("Authorization: Bearer secret\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Actor: admin\r\n"), "{request}");
        assert!(
            request.ends_with(find("healthcare").unwrap().source),
            "body must be the embedded pack"
        );
    }

    #[test]
    fn deploy_puts_local_file_pack_to_tenant_endpoint() {
        let pack = find("healthcare").unwrap().source;
        let path = write_temp_policy(pack);
        let (server, rx) = spawn_policy_server("HTTP/1.1 200 OK", r#"{"tenant":"acme"}"#);
        let code = deploy(None, Some(&path), &server, "acme", Some("secret"), None);
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("PUT /security/policy HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.ends_with(pack),
            "body must be the local policy file contents"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn deploy_reports_server_error_status() {
        let (server, _rx) = spawn_policy_server("HTTP/1.1 400 Bad Request", r#"{"error":"bad"}"#);
        let code = deploy(
            Some("healthcare"),
            None,
            &server,
            "acme",
            Some("secret"),
            None,
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn undeploy_deletes_tenant_endpoint() {
        let (server, rx) =
            spawn_policy_server("HTTP/1.1 200 OK", r#"{"tenant":"acme","deleted":true}"#);
        let code = undeploy(&server, "acme", Some("secret"), Some("admin"));
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("DELETE /security/policy HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.contains("Authorization: Bearer secret\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Actor: admin\r\n"), "{request}");
    }

    #[test]
    fn undeploy_reports_server_error_status() {
        let (server, _rx) = spawn_policy_server("HTTP/1.1 404 Not Found", r#"{"error":"missing"}"#);
        let code = undeploy(&server, "acme", Some("secret"), None);
        assert_eq!(code, 1);
    }

    #[test]
    fn undeploy_requires_tenant_and_token() {
        assert_eq!(
            undeploy("http://127.0.0.1:1", "  ", Some("secret"), None),
            1,
            "empty tenant must fail locally"
        );
        unsafe {
            std::env::remove_var("XAZZ_ADMIN_TOKEN");
            std::env::remove_var("XAZZ_SERVER_TOKEN");
        }
        assert_eq!(
            undeploy("http://127.0.0.1:1", "acme", None, None),
            1,
            "missing token must fail locally"
        );
    }
}
