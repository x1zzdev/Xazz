// src/policy_query.rs — read-only tenant policy queries (issue C2)
//
// `xazz registry deploy` writes a tenant policy pack; these commands read the
// tenant's policy state back from a running server:
//
//   GET /security/policy/history             — policy-pack change history
//   GET /security/policy/history/ttl         — effective history retention window
//   GET /security/policy/history/ttl/history — retention-window change history
//
// The CLI stays Tokio/Polars-free (see CONTRIBUTING), so requests go through the
// std-only `http` client. All three endpoints are tenant-scoped and read-only.

use serde_json::Value;

use crate::cli::PolicyView;
use crate::http;

/// `GET /security/policy/history` on `server`.
pub fn history_endpoint(server: &str) -> String {
    format!("{}/security/policy/history", base(server))
}

/// `GET /security/policy/history/ttl` on `server`.
pub fn ttl_endpoint(server: &str) -> String {
    format!("{}/security/policy/history/ttl", base(server))
}

/// `GET /security/policy/history/ttl/history` on `server`.
pub fn ttl_history_endpoint(server: &str) -> String {
    format!("{}/security/policy/history/ttl/history", base(server))
}

/// Trims `server` and strips any trailing slash so endpoint joins are stable.
fn base(server: &str) -> &str {
    server.trim().trim_end_matches('/')
}

/// Appends `?cursor=N&limit=M` for the paged history views.
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

/// `xazz policy-status <view> --tenant T [--server URL] [--token TOK] [--cursor ID] [--limit N] [--json]`
///
/// Fetches one read-only policy view for the tenant and prints it. With `--json`
/// the server body is echoed verbatim; otherwise a short human summary is printed
/// (falling back to the raw body if it is not JSON).
#[allow(clippy::too_many_arguments)]
pub fn run(
    view: PolicyView,
    server: &str,
    tenant: &str,
    token: Option<&str>,
    cursor: Option<i64>,
    limit: Option<usize>,
    json: bool,
) -> i32 {
    let tenant = tenant.trim();
    if tenant.is_empty() {
        eprintln!("[xazz] policy-status: --tenant must not be empty");
        return 1;
    }
    if server.trim().is_empty() {
        eprintln!("[xazz] policy-status: --server must not be empty");
        return 1;
    }

    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(crate::registry::env_token);
    let Some(token) = token else {
        eprintln!(
            "[xazz] policy-status: no token — pass --token or set XAZZ_ADMIN_TOKEN / XAZZ_SERVER_TOKEN"
        );
        return 1;
    };

    let endpoint = match view {
        PolicyView::History => with_paging(history_endpoint(server), cursor, limit),
        PolicyView::Ttl => ttl_endpoint(server),
        PolicyView::TtlHistory => with_paging(ttl_history_endpoint(server), cursor, limit),
    };
    let headers = [
        ("Authorization", format!("Bearer {token}")),
        ("X-Xazz-Tenant", tenant.to_string()),
    ];

    match http::get_json(&endpoint, &headers) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            if json {
                println!("{}", resp.body.trim());
            } else {
                print_summary(view, &resp.body);
            }
            0
        }
        Ok(resp) => {
            eprintln!(
                "[xazz] policy-status: server returned HTTP {} for tenant '{}'",
                resp.status, tenant
            );
            let body = resp.body.trim();
            if !body.is_empty() {
                eprintln!("        {body}");
            }
            1
        }
        Err(e) => {
            eprintln!("[xazz] policy-status: request failed — {e}");
            1
        }
    }
}

/// Prints a short human summary of a successful response body.
fn print_summary(view: PolicyView, body: &str) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        // A non-JSON body is unexpected, but echo it rather than swallow it.
        println!("{}", body.trim());
        return;
    };

    let field = |key: &str| value.get(key).cloned().unwrap_or(Value::Null);
    match view {
        PolicyView::Ttl => {
            println!("tenant     : {}", field("tenant"));
            println!("ttl_secs   : {}", field("ttl_secs"));
            println!("ttl_source : {}", field("ttl_source"));
        }
        PolicyView::History => {
            println!("tenant      : {}", field("tenant"));
            println!(
                "ttl_secs    : {} ({})",
                field("ttl_secs"),
                field("ttl_source")
            );
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
                    "#{} {} by {} at {}",
                    record.get("id").unwrap_or(&Value::Null),
                    record.get("action").unwrap_or(&Value::Null),
                    record.get("changed_by").unwrap_or(&Value::Null),
                    record.get("changed_at").unwrap_or(&Value::Null),
                );
            }
        }
        PolicyView::TtlHistory => {
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
                    "#{} {} ttl {} → {} by {} at {}",
                    record.get("id").unwrap_or(&Value::Null),
                    record.get("action").unwrap_or(&Value::Null),
                    record.get("old_ttl_secs").unwrap_or(&Value::Null),
                    record.get("new_ttl_secs").unwrap_or(&Value::Null),
                    record.get("changed_by").unwrap_or(&Value::Null),
                    record.get("changed_at").unwrap_or(&Value::Null),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_join_base_and_trim_slash() {
        assert_eq!(
            history_endpoint("http://127.0.0.1:8005"),
            "http://127.0.0.1:8005/security/policy/history"
        );
        assert_eq!(
            ttl_endpoint(" http://host/api/ "),
            "http://host/api/security/policy/history/ttl"
        );
        assert_eq!(
            ttl_history_endpoint("http://host"),
            "http://host/security/policy/history/ttl/history"
        );
    }

    #[test]
    fn paging_query_is_omitted_when_empty() {
        let e = history_endpoint("http://h");
        assert_eq!(with_paging(e.clone(), None, None), e);
    }

    #[test]
    fn paging_query_lists_cursor_then_limit() {
        let e = history_endpoint("http://h");
        assert_eq!(
            with_paging(e.clone(), Some(42), Some(10)),
            format!("{e}?cursor=42&limit=10")
        );
        assert_eq!(
            with_paging(e.clone(), Some(42), None),
            format!("{e}?cursor=42")
        );
        assert_eq!(
            with_paging(e.clone(), None, Some(10)),
            format!("{e}?limit=10")
        );
    }

    #[test]
    fn summary_falls_back_to_raw_body_when_not_json() {
        // `--json` echoes the body verbatim; the human path must not panic on a
        // non-JSON error page either.
        print_summary(PolicyView::History, "not-json");
    }

    /// Minimal one-shot HTTP server: captures the raw request and replies with the
    /// given status line/body. Lets `run` be verified end-to-end without a real
    /// xazz-server.
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
            loop {
                let n = stream.read(&mut chunk).expect("read request");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
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
    fn run_gets_ttl_endpoint_with_tenant_header() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","ttl_secs":3600,"ttl_source":"global"}"#,
        );
        let code = run(
            PolicyView::Ttl,
            &server,
            "acme",
            Some("secret"),
            None,
            None,
            false,
        );
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("GET /security/policy/history/ttl HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("X-Xazz-Tenant: acme\r\n"), "{request}");
        assert!(
            request.contains("Authorization: Bearer secret\r\n"),
            "{request}"
        );
    }

    #[test]
    fn run_pages_history_with_cursor_and_limit() {
        let (server, rx) = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"tenant":"acme","limit":10,"offset":0,"cursor":42,"next_cursor":null,"history":[]}"#,
        );
        let code = run(
            PolicyView::History,
            &server,
            "acme",
            Some("secret"),
            Some(42),
            Some(10),
            false,
        );
        assert_eq!(code, 0);

        let request = rx.recv().expect("captured request");
        assert!(
            request.starts_with("GET /security/policy/history?cursor=42&limit=10 HTTP/1.1\r\n"),
            "{request}"
        );
    }

    #[test]
    fn run_returns_error_on_non_2xx() {
        let (server, _rx) = spawn_server("HTTP/1.1 401 Unauthorized", r#"{"error":"no"}"#);
        let code = run(
            PolicyView::History,
            &server,
            "acme",
            Some("secret"),
            None,
            None,
            true,
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn run_requires_tenant_and_token() {
        assert_eq!(
            run(
                PolicyView::History,
                "http://127.0.0.1:1",
                "  ",
                Some("s"),
                None,
                None,
                false
            ),
            1
        );
        // No explicit token and no env tokens.
        unsafe {
            std::env::remove_var("XAZZ_ADMIN_TOKEN");
            std::env::remove_var("XAZZ_SERVER_TOKEN");
        }
        assert_eq!(
            run(
                PolicyView::History,
                "http://127.0.0.1:1",
                "acme",
                None,
                None,
                None,
                false
            ),
            1
        );
    }
}
