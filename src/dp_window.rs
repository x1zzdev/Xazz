// src/dp_window.rs — per-tenant DP budget window CLI (issue C2)
//
// `xazz dp window` manages a tenant's differential-privacy budget window
// override through a running server:
//
//   PUT    /dp/budget/window          — store an override (`window_secs`)
//   DELETE /dp/budget/window          — clear the override (global fallback)
//   GET    /dp/budget/window/history  — append-only change log
//
// The CLI stays Tokio/Polars-free (see CONTRIBUTING), so requests go through the
// std-only `http` client. Every call is tenant-scoped and accepts an admin actor
// (`X-Xazz-Actor`) for a delegated change.

use serde_json::Value;

use crate::http;
use crate::registry::env_token;

/// `PUT /dp/budget/window` on `server`.
pub fn window_endpoint(server: &str) -> String {
    format!("{}/dp/budget/window", base(server))
}

/// `GET /dp/budget/window/history` on `server`.
pub fn window_history_endpoint(server: &str) -> String {
    format!("{}/dp/budget/window/history", base(server))
}

/// Trims `server` and strips any trailing slash so endpoint joins are stable.
fn base(server: &str) -> &str {
    server.trim().trim_end_matches('/')
}

/// Appends `?cursor=N&limit=M` for the paged history view.
fn with_paging(endpoint: String, cursor: Option<i64>, limit: Option<usize>) -> String {
    let mut params: Vec<String> = Vec::new();
    if let Some(cursor) = cursor {
        params.push(format!("cursor={cursor}"));
    }
    if let Some(limit) = limit {
        params.push(format!("limit={limit}"));
    }
    if params.is_empty() {
        endpoint
    } else {
        format!("{endpoint}?{}", params.join("&"))
    }
}

/// `xazz dp window set --window-secs N --tenant T [--server URL] [--token TOK] [--actor A]`
///
/// Stores the tenant's DP budget window override through the server's
/// `PUT /dp/budget/window` endpoint (issue C2). `window_secs: 0` stores an explicit
/// "cumulative, no window" override (distinct from clearing it). Validation mirrors
/// the policy commands, so an empty tenant/server or a missing token fails locally
/// before any network call.
pub fn set(
    server: &str,
    tenant: &str,
    window_secs: u64,
    token: Option<&str>,
    actor: Option<&str>,
) -> i32 {
    let (tenant, token) = match prepare(server, tenant, token, "set") {
        Ok(parts) => parts,
        Err(code) => return code,
    };
    let endpoint = window_endpoint(server);
    let headers = auth_headers(&token, tenant, actor);
    let body = serde_json::json!({ "window_secs": window_secs }).to_string();

    match http::put_json(&endpoint, &headers, &body) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            print_effective("set", tenant, &endpoint, &resp.body);
            0
        }
        Ok(resp) => report_status_error(resp.status, tenant, &resp.body),
        Err(e) => {
            eprintln!("[xazz] dp window: set failed — {e}");
            1
        }
    }
}

/// `xazz dp window clear --tenant T [--server URL] [--token TOK] [--actor A]`
///
/// Removes the tenant's DP budget window override through the server's
/// `DELETE /dp/budget/window` endpoint (issue C2); the tenant then falls back to
/// the global `XAZZ_TENANT_DP_WINDOW_SECS` default. Validation mirrors [`set`].
pub fn clear(server: &str, tenant: &str, token: Option<&str>, actor: Option<&str>) -> i32 {
    let (tenant, token) = match prepare(server, tenant, token, "clear") {
        Ok(parts) => parts,
        Err(code) => return code,
    };
    let endpoint = window_endpoint(server);
    let headers = auth_headers(&token, tenant, actor);

    match http::delete_json(&endpoint, &headers) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            print_effective("clear", tenant, &endpoint, &resp.body);
            0
        }
        Ok(resp) => report_status_error(resp.status, tenant, &resp.body),
        Err(e) => {
            eprintln!("[xazz] dp window: clear failed — {e}");
            1
        }
    }
}

/// `xazz dp window history --tenant T [--server URL] [--token TOK] [--cursor ID] [--limit N] [--json]`
///
/// Reads the tenant's append-only DP budget window override change log through the
/// server's `GET /dp/budget/window/history` endpoint (issue C2). With `--json` the
/// server body is echoed verbatim; otherwise a short human summary is printed.
pub fn history(
    server: &str,
    tenant: &str,
    token: Option<&str>,
    cursor: Option<i64>,
    limit: Option<usize>,
    json: bool,
) -> i32 {
    let (tenant, token) = match prepare(server, tenant, token, "history") {
        Ok(parts) => parts,
        Err(code) => return code,
    };
    let endpoint = with_paging(window_history_endpoint(server), cursor, limit);
    let headers = auth_headers(&token, tenant, None);

    match http::get_json(&endpoint, &headers) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            if json {
                println!("{}", resp.body.trim());
            } else {
                print_history(&resp.body);
            }
            0
        }
        Ok(resp) => report_status_error(resp.status, tenant, &resp.body),
        Err(e) => {
            eprintln!("[xazz] dp window: history failed — {e}");
            1
        }
    }
}

/// Validates the shared arguments and returns the trimmed `(tenant, token)`.
///
/// `verb` only labels the error output (`set`/`clear`/`history`).
fn prepare<'a>(
    server: &str,
    tenant: &'a str,
    token: Option<&str>,
    verb: &str,
) -> Result<(&'a str, String), i32> {
    let tenant = tenant.trim();
    if tenant.is_empty() {
        eprintln!("[xazz] dp window {verb}: --tenant must not be empty");
        return Err(1);
    }
    if server.trim().is_empty() {
        eprintln!("[xazz] dp window {verb}: --server must not be empty");
        return Err(1);
    }

    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(env_token);
    let Some(token) = token else {
        eprintln!(
            "[xazz] dp window {verb}: no token — pass --token or set XAZZ_ADMIN_TOKEN / XAZZ_SERVER_TOKEN"
        );
        return Err(1);
    };
    Ok((tenant, token))
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
    println!("✔ {action} DP budget window for tenant '{tenant}'");
    println!("  server: {endpoint}");
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let field = |key: &str| value.get(key).cloned().unwrap_or(Value::Null);
        println!("  window_secs   : {}", field("window_secs"));
        println!("  window_source : {}", field("window_source"));
    }
}

/// Prints a short human summary of a history response body.
fn print_history(body: &str) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        // A non-JSON body is unexpected, but echo it rather than swallow it.
        println!("{}", body.trim());
        return;
    };

    let field = |key: &str| value.get(key).cloned().unwrap_or(Value::Null);
    println!("tenant      : {}", field("tenant"));
    println!("limit/offset: {} / {}", field("limit"), field("offset"));
    println!("next_cursor : {}", field("next_cursor"));
    println!("─────────────────────────────────────────────");
    for record in value
        .get("history")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        println!(
            "#{} {} window {} → {} by {} at {}",
            record.get("id").unwrap_or(&Value::Null),
            record.get("action").unwrap_or(&Value::Null),
            record.get("old_window_secs").unwrap_or(&Value::Null),
            record.get("new_window_secs").unwrap_or(&Value::Null),
            record.get("changed_by").unwrap_or(&Value::Null),
            record.get("changed_at").unwrap_or(&Value::Null),
        );
    }
}

/// Reports a non-2xx response, echoing the server body when present.
fn report_status_error(status: u16, tenant: &str, body: &str) -> i32 {
    eprintln!("[xazz] dp window: server returned HTTP {status} for tenant '{tenant}'");
    let body = body.trim();
    if !body.is_empty() {
        eprintln!("        {body}");
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_join_base_and_trim_slash() {
        assert_eq!(
            window_endpoint("http://127.0.0.1:8005"),
            "http://127.0.0.1:8005/dp/budget/window"
        );
        assert_eq!(
            window_endpoint(" http://host/api/ "),
            "http://host/api/dp/budget/window"
        );
        assert_eq!(
            window_history_endpoint("http://host"),
            "http://host/dp/budget/window/history"
        );
    }

    #[test]
    fn paging_query_lists_cursor_then_limit() {
        let e = window_history_endpoint("http://h");
        assert_eq!(with_paging(e.clone(), None, None), e);
        assert_eq!(
            with_paging(e.clone(), Some(42), Some(10)),
            format!("{e}?cursor=42&limit=10")
        );
        assert_eq!(
            with_paging(e.clone(), Some(42), None),
            format!("{e}?cursor=42")
        );
    }

    /// Minimal one-shot HTTP server that captures the raw request (headers and
    /// body) and replies with the given status line/body. Lets the commands be
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
    fn set_puts_window_json_to_tenant_endpoint() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","window_secs":86400,"window_source":"tenant"}"#,
        );
        let code = set(&server, "acme", 86400, Some("secret"), Some("admin"));
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("PUT /dp/budget/window HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.contains("Authorization: Bearer secret\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Actor: admin\r\n"), "{request}");
        assert!(
            request.ends_with(r#"{"window_secs":86400}"#),
            "body must carry the requested window: {request}"
        );
    }

    #[test]
    fn clear_deletes_tenant_endpoint() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","window_secs":0,"window_source":"global"}"#,
        );
        let code = clear(&server, "acme", Some("secret"), Some("admin"));
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("DELETE /dp/budget/window HTTP/1.1\r\n"),
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
    fn history_gets_endpoint_with_cursor_and_limit() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","limit":10,"offset":0,"cursor":42,"next_cursor":null,"history":[]}"#,
        );
        let code = history(&server, "acme", Some("secret"), Some(42), Some(10), false);
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("GET /dp/budget/window/history?cursor=42&limit=10 HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.contains("Authorization: Bearer secret\r\n"),
            "{request}"
        );
    }

    #[test]
    fn reports_server_error_status() {
        let (server, _rx) = spawn_server("HTTP/1.1 400 Bad Request", r#"{"error":"bad"}"#);
        assert_eq!(set(&server, "acme", 1, Some("secret"), None), 1);

        let (server, _rx) = spawn_server("HTTP/1.1 404 Not Found", r#"{"error":"missing"}"#);
        assert_eq!(clear(&server, "acme", Some("secret"), None), 1);

        let (server, _rx) = spawn_server("HTTP/1.1 401 Unauthorized", r#"{"error":"no"}"#);
        assert_eq!(
            history(&server, "acme", Some("secret"), None, None, true),
            1
        );
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
        assert_eq!(
            history("  ", "acme", Some("secret"), None, None, false),
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
        assert_eq!(
            history("http://127.0.0.1:1", "acme", None, None, None, false),
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

    #[test]
    fn history_summary_falls_back_to_raw_body_when_not_json() {
        print_history("not-json");
    }
}
