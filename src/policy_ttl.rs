// src/policy_ttl.rs — tenant policy-history retention-window set/clear (issue C2)
//
// `xazz policy-status ttl` reads a tenant's effective history retention window;
// these commands change the per-tenant override so history older than the window
// is pruned:
//
//   PUT    /security/policy/history/ttl — store an override (`ttl_secs`)
//   DELETE /security/policy/history/ttl — clear the override (global fallback)
//
// The CLI stays Tokio/Polars-free (see CONTRIBUTING), so requests go through the
// std-only `http` client. Both calls are tenant-scoped and accept an admin actor
// (`X-Xazz-Actor`) for a delegated change.

use serde_json::Value;

use crate::http;
use crate::policy_query::ttl_endpoint;
use crate::registry::env_token;

/// `xazz policy-ttl set --ttl-secs N --tenant T [--server URL] [--token TOK] [--actor A]`
///
/// Stores the tenant's policy-history retention-window override through the
/// server's `PUT /security/policy/history/ttl` endpoint (issue C2). `ttl_secs: 0`
/// stores an explicit "keep forever" override. Validation mirrors the registry
/// commands, so an empty tenant/server or a missing token fails locally before
/// any network call.
pub fn set(
    server: &str,
    tenant: &str,
    ttl_secs: u64,
    token: Option<&str>,
    actor: Option<&str>,
) -> i32 {
    let (endpoint, token) = match prepare(server, tenant, token, "set") {
        Ok(parts) => parts,
        Err(code) => return code,
    };
    let headers = auth_headers(&token, tenant, actor);
    let body = serde_json::json!({ "ttl_secs": ttl_secs }).to_string();

    match http::put_json(&endpoint, &headers, &body) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            print_effective("set", tenant, &endpoint, &resp.body);
            0
        }
        Ok(resp) => report_status_error(resp.status, tenant, &resp.body),
        Err(e) => {
            eprintln!("[xazz] policy-ttl: set failed — {e}");
            1
        }
    }
}

/// `xazz policy-ttl clear --tenant T [--server URL] [--token TOK] [--actor A]`
///
/// Removes the tenant's policy-history retention-window override through the
/// server's `DELETE /security/policy/history/ttl` endpoint (issue C2); the tenant
/// then falls back to the global default. Validation mirrors [`set`].
pub fn clear(server: &str, tenant: &str, token: Option<&str>, actor: Option<&str>) -> i32 {
    let (endpoint, token) = match prepare(server, tenant, token, "clear") {
        Ok(parts) => parts,
        Err(code) => return code,
    };
    let headers = auth_headers(&token, tenant, actor);

    match http::delete_json(&endpoint, &headers) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            print_effective("clear", tenant, &endpoint, &resp.body);
            0
        }
        Ok(resp) => report_status_error(resp.status, tenant, &resp.body),
        Err(e) => {
            eprintln!("[xazz] policy-ttl: clear failed — {e}");
            1
        }
    }
}

/// Validates the shared arguments and returns `(endpoint, token)`.
///
/// `verb` only labels the error output (`set`/`clear`).
fn prepare(
    server: &str,
    tenant: &str,
    token: Option<&str>,
    verb: &str,
) -> Result<(String, String), i32> {
    let tenant = tenant.trim();
    if tenant.is_empty() {
        eprintln!("[xazz] policy-ttl {verb}: --tenant must not be empty");
        return Err(1);
    }
    if server.trim().is_empty() {
        eprintln!("[xazz] policy-ttl {verb}: --server must not be empty");
        return Err(1);
    }

    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(env_token);
    let Some(token) = token else {
        eprintln!(
            "[xazz] policy-ttl {verb}: no token — pass --token or set XAZZ_ADMIN_TOKEN / XAZZ_SERVER_TOKEN"
        );
        return Err(1);
    };
    Ok((ttl_endpoint(server), token))
}

/// Builds the tenant/auth headers, adding `X-Xazz-Actor` when an admin actor is set.
fn auth_headers(token: &str, tenant: &str, actor: Option<&str>) -> Vec<(&'static str, String)> {
    let mut headers: Vec<(&'static str, String)> = vec![
        ("Authorization", format!("Bearer {token}")),
        ("X-Xazz-Tenant", tenant.trim().to_string()),
    ];
    if let Some(actor) = actor.map(str::trim).filter(|value| !value.is_empty()) {
        headers.push(("X-Xazz-Actor", actor.to_string()));
    }
    headers
}

/// Prints the effective window reported by the server (or a short success line
/// when the body is not JSON).
fn print_effective(action: &str, tenant: &str, endpoint: &str, body: &str) {
    println!("✔ {action} policy-history TTL for tenant '{tenant}'");
    println!("  server: {endpoint}");
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let field = |key: &str| value.get(key).cloned().unwrap_or(Value::Null);
        println!("  ttl_secs   : {}", field("ttl_secs"));
        println!("  ttl_source : {}", field("ttl_source"));
    }
}

/// Reports a non-2xx response, echoing the server body when present.
fn report_status_error(status: u16, tenant: &str, body: &str) -> i32 {
    eprintln!("[xazz] policy-ttl: server returned HTTP {status} for tenant '{tenant}'");
    let body = body.trim();
    if !body.is_empty() {
        eprintln!("        {body}");
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal one-shot HTTP server that captures the raw request (headers and
    /// body) and replies with the given status line/body. Lets `set`/`clear` be
    /// verified end-to-end without a real xazz-server.
    fn spawn_server(
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
    fn set_puts_ttl_json_to_tenant_endpoint() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","ttl_secs":86400,"ttl_source":"tenant"}"#,
        );
        let code = set(&server, "acme", 86400, Some("secret"), Some("admin"));
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("PUT /security/policy/history/ttl HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.contains("Authorization: Bearer secret\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Actor: admin\r\n"), "{request}");
        assert!(
            request.ends_with(r#"{"ttl_secs":86400}"#),
            "body must carry the requested window: {request}"
        );
    }

    #[test]
    fn clear_deletes_tenant_endpoint() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","ttl_secs":3600,"ttl_source":"global"}"#,
        );
        let code = clear(&server, "acme", Some("secret"), Some("admin"));
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("DELETE /security/policy/history/ttl HTTP/1.1\r\n"),
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
    fn set_reports_server_error_status() {
        let (server, _rx) = spawn_server("HTTP/1.1 400 Bad Request", r#"{"error":"bad"}"#);
        let code = set(&server, "acme", 1, Some("secret"), None);
        assert_eq!(code, 1);
    }

    #[test]
    fn clear_reports_server_error_status() {
        let (server, _rx) = spawn_server("HTTP/1.1 404 Not Found", r#"{"error":"missing"}"#);
        let code = clear(&server, "acme", Some("secret"), None);
        assert_eq!(code, 1);
    }

    #[test]
    fn requires_tenant_server_and_token() {
        assert_eq!(
            set("http://127.0.0.1:1", "  ", 1, Some("secret"), None),
            1,
            "empty tenant must fail locally"
        );
        assert_eq!(
            clear("  ", "acme", Some("secret"), None),
            1,
            "empty server must fail locally"
        );
        unsafe {
            std::env::remove_var("XAZZ_ADMIN_TOKEN");
            std::env::remove_var("XAZZ_SERVER_TOKEN");
        }
        assert_eq!(
            set("http://127.0.0.1:1", "acme", 1, None, None),
            1,
            "missing token must fail locally"
        );
        assert_eq!(
            clear("http://127.0.0.1:1", "acme", None, None),
            1,
            "missing token must fail locally"
        );
    }

    #[test]
    fn auth_headers_omit_blank_actor() {
        let headers = auth_headers("tok", " acme ", Some("  "));
        assert_eq!(headers.len(), 2, "blank actor must not add a header");
        assert_eq!(headers[1].1, "acme", "tenant is trimmed");
        assert!(!headers.iter().any(|(name, _)| *name == "X-Xazz-Actor"));
    }
}
