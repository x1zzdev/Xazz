// src/http.rs — minimal blocking HTTP/1.1 client for the CLI (issue C2)
//
// The `xazz` CLI deliberately stays free of Tokio/Polars (see CONTRIBUTING), and
// the server it talks to (`xazz-server`) is a loopback service by default. So a
// tiny std-only client is enough for `xazz registry deploy`/`undeploy`, which
// issue a single `PUT`/`DELETE /security/policy` request.
//
// Scope: `http://` only. TLS termination belongs in the reverse proxy that fronts
// a remote server, so an `https://` URL is rejected with a clear message rather
// than silently downgraded.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// A parsed HTTP response — the status code and decoded body.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// Parsed `http://host[:port]/path` target.
#[derive(Debug)]
struct Target {
    host: String,
    port: u16,
    path: String,
    host_header: String,
}

impl Target {
    fn parse(url: &str) -> Result<Self, String> {
        let url = url.trim();
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| format!("invalid server URL '{url}': expected http://host[:port]"))?;
        if !scheme.eq_ignore_ascii_case("http") {
            return Err(if scheme.eq_ignore_ascii_case("https") {
                "https:// is not supported by `xazz registry deploy`; terminate TLS at a \
                 reverse proxy and pass the http:// origin"
                    .to_string()
            } else {
                format!("unsupported URL scheme '{scheme}' (only http://)")
            });
        }

        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(format!("invalid server URL '{url}': missing host"));
        }

        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| format!("invalid port '{port}' in '{url}'"))?;
                (host.to_string(), port)
            }
            Some(_) => return Err(format!("invalid server URL '{url}': missing host")),
            None => (authority.to_string(), 80),
        };
        let host_header = if port == 80 {
            host.clone()
        } else {
            format!("{host}:{port}")
        };

        Ok(Target {
            host,
            port,
            path: path.to_string(),
            host_header,
        })
    }
}

/// Sends `GET <url>` with the given extra headers, returning the response.
///
/// Used by the read-only policy queries (`xazz policy-status`, issue C2).
pub fn get_json(url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
    request("GET", url, headers, None)
}

/// Sends `PUT <url>` with `body` and the given extra headers, returning the
/// response. The connection uses `Connection: close`, so the body is read to EOF.
pub fn put_json(url: &str, headers: &[(&str, String)], body: &str) -> Result<Response, String> {
    request("PUT", url, headers, Some(body))
}

/// Sends `DELETE <url>` with the given extra headers, returning the response.
///
/// Used by `xazz registry undeploy` to remove a tenant's policy pack
/// (`DELETE /security/policy`, issue C2).
pub fn delete_json(url: &str, headers: &[(&str, String)]) -> Result<Response, String> {
    request("DELETE", url, headers, None)
}

/// Sends a single HTTP/1.1 request (`GET`/`DELETE` without a body, `PUT` with
/// one) and parses the response. TLS is out of scope — see the module docs.
fn request(
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: Option<&str>,
) -> Result<Response, String> {
    let target = Target::parse(url)?;
    for (name, value) in headers {
        if value.contains('\r') || value.contains('\n') {
            return Err(format!(
                "refusing header '{name}': value contains a newline"
            ));
        }
    }

    let mut stream = TcpStream::connect((target.host.as_str(), target.port))
        .map_err(|e| format!("cannot connect to {}:{} — {e}", target.host, target.port))?;
    let timeout = Some(Duration::from_secs(30));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);

    let mut request = String::with_capacity(body.map_or(0, str::len) + 256);
    request.push_str(&format!("{method} {} HTTP/1.1\r\n", target.path));
    request.push_str(&format!("Host: {}\r\n", target.host_header));
    request.push_str("Accept: application/json\r\n");
    request.push_str("Connection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    match body {
        Some(body) => {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
            request.push_str(body);
        }
        None => request.push_str("\r\n"),
    }

    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("failed to send request — {e}"))?;
    stream
        .flush()
        .map_err(|e| format!("failed to send request — {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("failed to read response — {e}"))?;
    parse_response(&String::from_utf8_lossy(&raw))
}

/// Parses the status line, decodes a `Transfer-Encoding: chunked` body if present,
/// and returns the response.
fn parse_response(text: &str) -> Result<Response, String> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response (no header terminator)".to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "malformed HTTP status line".to_string())?;

    let chunked = head.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        line.starts_with("transfer-encoding:") && line.contains("chunked")
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_string()
    };
    Ok(Response { status, body })
}

/// Decodes an HTTP/1.1 chunked transfer body.
fn dechunk(raw: &str) -> Result<String, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut rest = raw.as_bytes();
    loop {
        let pos =
            find_subslice(rest, b"\r\n").ok_or_else(|| "malformed chunked body".to_string())?;
        let size_line = std::str::from_utf8(&rest[..pos]).map_err(|_| "malformed chunk size")?;
        let size =
            usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| "malformed chunk size".to_string())?;
        rest = &rest[pos + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size {
            return Err("truncated chunk".to_string());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size..];
        if rest.starts_with(b"\r\n") {
            rest = &rest[2..];
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_target_with_default_and_explicit_port() {
        let t = Target::parse("http://127.0.0.1/security/policy").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.path.as_str()),
            ("127.0.0.1", 80, "/security/policy")
        );
        assert_eq!(t.host_header, "127.0.0.1");

        let t = Target::parse("http://example.test:8005/security/policy").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("example.test", 8005));
        assert_eq!(t.host_header, "example.test:8005");

        let t = Target::parse("http://example.test:8005").unwrap();
        assert_eq!(t.path, "/");
    }

    #[test]
    fn rejects_https_and_unknown_schemes() {
        let err = Target::parse("https://example.test").unwrap_err();
        assert!(err.contains("https:// is not supported"), "{err}");
        let err = Target::parse("ftp://example.test").unwrap_err();
        assert!(err.contains("unsupported URL scheme"), "{err}");
        let err = Target::parse("example.test").unwrap_err();
        assert!(err.contains("expected http://host"), "{err}");
    }

    #[test]
    fn parses_status_and_chunked_body() {
        let plain = parse_response("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(plain.status, 200);
        assert_eq!(plain.body, "{}");

        let chunked =
            parse_response("HTTP/1.1 400 Bad Request\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n2\r\n1}\r\n0\r\n\r\n")
                .unwrap();
        assert_eq!(chunked.status, 400);
        assert_eq!(chunked.body, "{\"a\":1}");
    }

    #[test]
    fn rejects_header_injection() {
        let err = put_json(
            "http://127.0.0.1:1/",
            &[("X-Test", "ok\r\nInjected: yes".to_string())],
            "{}",
        )
        .unwrap_err();
        assert!(err.contains("contains a newline"), "{err}");
    }
}
