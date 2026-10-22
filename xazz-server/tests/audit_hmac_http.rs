//! Real server process tests: exercise production env loading and HTTP handlers,
//! using temporary logs/DBs and per-child settings (no global env mutation).
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    addr: SocketAddr,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn spawn(dir: &Path, settings: &[(&str, String)], starts: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_xazz-server"));
        // Don't inherit the operator's auth/DB/path settings or audit keys.
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("XAZZ_")
            {
                cmd.env_remove(key);
            }
        }
        cmd.current_dir(dir)
            .env("XAZZ_BIND", addr.to_string())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("server.stderr")).unwrap());
        for (key, value) in settings {
            cmd.env(key, value);
        }
        drop(listener);
        let mut server = Self {
            child: cmd.spawn().unwrap(),
            addr,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = server.child.try_wait().unwrap() {
                assert!(
                    !starts && !status.success(),
                    "unexpected server exit: {status}: {}",
                    std::fs::read_to_string(dir.join("server.stderr")).unwrap_or_default()
                );
                return server;
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
                assert!(starts, "invalid settings/history must prevent startup");
                return server;
            }
            assert!(
                Instant::now() < deadline,
                "server startup/exit timed out: {}",
                std::fs::read_to_string(dir.join("server.stderr")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn request(&self, path: &str, body: Option<Value>) -> (u16, String) {
        let mut stream = TcpStream::connect(self.addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let method = if body.is_some() { "POST" } else { "GET" };
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", self.addr, body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        (
            headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
            body.into(),
        )
    }

    fn json(&self, path: &str, body: Option<Value>) -> Value {
        let (status, response) = self.request(path, body);
        assert_eq!(status, 200, "{response}");
        serde_json::from_str(&response).unwrap()
    }
}

fn settings(active: &str, boundary: u64) -> Vec<(&'static str, String)> {
    vec![
        (
            "XAZZ_AUDIT_HMAC_KEYS",
            json!({"old": "ab".repeat(32), "new": "cd".repeat(32)}).to_string(),
        ),
        ("XAZZ_AUDIT_HMAC_ACTIVE_KEY_ID", active.into()),
        ("XAZZ_AUDIT_HMAC_REQUIRED_FROM", boundary.to_string()),
    ]
}

#[test]
fn production_hmac_configuration_rotation_and_tamper_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit_log/audit.jsonl");
    let server = Server::spawn(dir.path(), &settings("old", 0), true);
    server.json("/security/audit", Some(json!({"code": "한글\ncode"})));
    let report = server.json("/security/audit/chain", None);
    assert_eq!(report["authenticated"], true);
    assert_eq!(report["records"], 1);
    let records = server.json("/security/audit/log", None);
    assert_eq!(records["records"][0]["hmac_key_id"], "old");
    assert!(!records.to_string().contains(&"ab".repeat(32)));
    drop(server);

    let server = Server::spawn(dir.path(), &settings("new", 0), true);
    server.json("/security/audit", Some(json!({"code": "rotated"})));
    assert_eq!(
        server.json("/security/audit/chain", None)["authenticated"],
        true
    );
    let records = server.json("/security/audit/log", None);
    assert_eq!(records["records"][0]["hmac_key_id"], "old");
    assert_eq!(records["records"][1]["hmac_key_id"], "new");

    // File-only attacker strips every authentication field. SHA hashes remain valid.
    let forged = records["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            let mut record = record.clone();
            record.as_object_mut().unwrap().remove("hmac");
            record.as_object_mut().unwrap().remove("hmac_key_id");
            record.to_string() + "\n"
        })
        .collect::<String>();
    std::fs::write(&path, &forged).unwrap();
    let report = server.json("/security/audit/chain", None);
    assert_eq!(report["intact"], false);
    assert_eq!(report["authenticated"], false);
    assert_eq!(
        server
            .request("/security/audit", Some(json!({"code": "must fail"})))
            .0,
        500
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), forged);
    drop(server);
    drop(Server::spawn(dir.path(), &settings("new", 0), false));
    assert!(
        std::fs::read_to_string(dir.path().join("server.stderr"))
            .unwrap()
            .contains("audit authentication failed at startup")
    );
}

#[test]
fn production_legacy_migration_and_invalid_config() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::spawn(dir.path(), &[], true);
    server.json("/security/audit", Some(json!({"code": "legacy"})));
    let report = server.json("/security/audit/chain", None);
    assert_eq!(report["intact"], true);
    assert_eq!(report["authenticated"], false);
    assert!(report["warning"].is_string());
    drop(server);
    assert!(
        std::fs::read_to_string(dir.path().join("server.stderr"))
            .unwrap()
            .contains("WARNING")
    );
    drop(Server::spawn(dir.path(), &settings("old", 0), false));
    let server = Server::spawn(dir.path(), &settings("old", 1), true);
    server.json("/security/audit", Some(json!({"code": "signed"})));
    let report = server.json("/security/audit/chain", None);
    assert_eq!(report["intact"], true);
    assert_eq!(report["legacy_records"], 1);
    assert_eq!(report["authenticated"], false);
    drop(server);
    // No-key restart cannot silently accept a previously signed chain.
    drop(Server::spawn(dir.path(), &[], false));
    assert!(
        std::fs::read_to_string(dir.path().join("server.stderr"))
            .unwrap()
            .contains("audit authentication failed at startup")
    );
    let invalid = [("XAZZ_AUDIT_HMAC_KEYS", "never-print-this-secret".into())];
    drop(Server::spawn(dir.path(), &invalid, false));
    assert!(
        !std::fs::read_to_string(dir.path().join("server.stderr"))
            .unwrap()
            .contains("never-print-this-secret")
    );
}

#[test]
fn two_server_processes_share_an_authenticated_chain() {
    let dir = tempfile::tempdir().unwrap();
    let first = Server::spawn(dir.path(), &settings("old", 0), true);
    let second = Server::spawn(dir.path(), &settings("old", 0), true);
    std::thread::scope(|scope| {
        for (writer, server) in [&first, &second].into_iter().enumerate() {
            scope.spawn(move || {
                for index in 0..8 {
                    server.json(
                        "/security/audit",
                        Some(json!({"code": format!("{writer}:{index}")})),
                    );
                    assert_eq!(server.json("/security/audit/chain", None)["intact"], true);
                }
            });
        }
    });
    let report = first.json("/security/audit/chain", None);
    assert_eq!(report["records"], 16);
    assert_eq!(report["authenticated"], true);
}
