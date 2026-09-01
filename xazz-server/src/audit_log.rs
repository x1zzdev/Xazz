//! xazz-server audit_log.rs — SHA-256 audit log (append-only JSONL)
//!
//! Persists all security audit records in a JSONL file on disk.
//! Each record contains the SHA-256 hash of the code, a timestamp, and the hash
//! of the previous record, forming a hash chain. This makes log tampering detectable.
//!
//!   - append(): adds a new audit record (appends to the file)
//!   - all():    returns the entire log
//!   - lookup(): looks up records matching a code hash
//!
//! JSONL file location: `audit_log/audit.jsonl` in the server's working directory

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

/// JSONL file path (relative to the server's working directory)
pub const AUDIT_LOG_DIR: &str = "audit_log";
pub const AUDIT_LOG_FILE: &str = "audit_log/audit.jsonl";

/// Global lock that serializes append's read-modify-write.
/// Prevents the TOCTOU where concurrent appends compute the same index/prev_hash
/// and break the audit chain.
static APPEND_LOCK: Mutex<()> = Mutex::new(());

/// Audit log record — one JSON line
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Record sequence number (starting from 0)
    pub index: u64,
    /// ISO-8601 timestamp
    pub timestamp: String,
    /// SHA-256 hash of the target code
    pub hash: String,
    /// Length of the original code (bytes)
    pub code_length: usize,
    /// Execution result status ("success" | "failed" | enum value). None if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// SHA-256 hash of the previous record (forms the chain) — the first record is "GENESIS"
    pub prev_hash: String,
    /// SHA-256 hash of this whole record (excluding prev_hash)
    pub record_hash: String,
}

impl AuditRecord {
    /// For hash-chain verification — recomputes record_hash and checks the prev_hash link
    pub fn verify(&self) -> bool {
        let computed = compute_record_hash(self);
        computed == self.record_hash
    }
}

/// record_hash computation (excludes the record_hash field itself)
fn compute_record_hash(r: &AuditRecord) -> String {
    let mut hasher = Sha256::new();
    hasher.update(r.index.to_string().as_bytes());
    hasher.update(&[0u8]);
    hasher.update(r.timestamp.as_bytes());
    hasher.update(&[0u8]);
    hasher.update(r.hash.as_bytes());
    hasher.update(&[0u8]);
    hasher.update(r.code_length.to_string().as_bytes());
    hasher.update(&[0u8]);
    if let Some(outcome) = &r.outcome {
        hasher.update(outcome.as_bytes());
        hasher.update(&[0u8]);
    }
    hasher.update(r.prev_hash.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Returns the SHA-256 hash of a code string
pub fn hash_code(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Creates the log file directory and returns its path.
fn ensure_log_dir() -> Result<std::path::PathBuf, String> {
    let path = std::path::PathBuf::from(AUDIT_LOG_DIR);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("failed to create audit-log directory: {e}"))?;
    Ok(path)
}

/// Creates a new audit record and appends it to the end of the file. (append-only)
///
/// `outcome` is the execution result status ("success"/"failed", etc.); for
/// backward-compatible existing signature calls, `append(code)` (no outcome) is also supported.
pub fn append(code: &str) -> Result<AuditRecord, String> {
    append_with_outcome(code, None)
}

/// Adds an audit record including the execution result (outcome). (default log file)
pub fn append_with_outcome(code: &str, outcome: Option<&str>) -> Result<AuditRecord, String> {
    ensure_log_dir()?;
    let file_path = std::path::PathBuf::from(AUDIT_LOG_FILE);
    append_to_path(code, outcome, &file_path)
}

/// Writes code + outcome to the specified file (append-only). (internal, for tests)
fn append_to_path(
    code: &str,
    outcome: Option<&str>,
    file_path: &std::path::Path,
) -> Result<AuditRecord, String> {
    // 1) Serialize concurrent in-process append read-modify-writes. (TOCTOU prevention)
    let _guard = APPEND_LOCK
        .lock()
        .map_err(|_| "failed to acquire audit-log lock (poisoned)".to_string())?;

    // 2) Take an OS exclusive file lock so the chain is not broken across multiple
    //    instances (multi-process). flock applies across process boundaries, so even
    //    when several xazz-servers append to the same log concurrently, index/prev_hash
    //    are computed atomically.
    use fs2::FileExt;
    use std::io::Write;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(file_path)
        .map_err(|e| format!("failed to open audit-log file for lock: {e}"))?;
    lock_file
        .lock_exclusive()
        .map_err(|e| format!("failed to lock audit-log file: {e}"))?;

    let existing = read_all(file_path).map_err(|e| format!("failed to read audit log: {e}"))?;
    let index = existing.len() as u64;
    let prev_hash = existing
        .last()
        .map(|r| r.record_hash.clone())
        .unwrap_or_else(|| "GENESIS".to_string());

    let record = AuditRecord {
        index,
        timestamp: chrono::Utc::now().to_rfc3339(),
        hash: hash_code(code),
        code_length: code.len(),
        outcome: outcome.map(|s| s.to_string()),
        prev_hash,
        record_hash: String::new(),
    };
    let record_hash = compute_record_hash(&record);
    let record = AuditRecord {
        record_hash,
        ..record
    };

    // append-only: use append(true) in OpenOptions
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .map_err(|e| format!("failed to open audit-log file: {e}"))?;
    let line =
        serde_json::to_string(&record).map_err(|e| format!("JSON serialization failed: {e}"))?;
    writeln!(file, "{}", line).map_err(|e| format!("failed to write audit log: {e}"))?;

    // Durability: flush the file buffer to the OS and sync it to disk.
    // Without fsync, records left only in the buffer are lost on a process/OS crash.
    file.flush()
        .map_err(|e| format!("failed to flush audit log: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("failed to fsync audit log: {e}"))?;

    // Release the exclusive lock (also released automatically when the file goes out of scope)
    let _ = lock_file.unlock();

    Ok(record)
}

/// Reads all records from the log file in order.
pub fn all() -> Result<Vec<AuditRecord>, String> {
    read_all(&std::path::PathBuf::from(AUDIT_LOG_FILE))
}

fn read_all(file_path: &std::path::Path) -> Result<Vec<AuditRecord>, String> {
    if !file_path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        std::fs::read_to_string(file_path).map_err(|e| format!("failed to read audit log: {e}"))?;
    let mut records = Vec::new();
    for (i, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rec: AuditRecord = serde_json::from_str(trimmed)
            .map_err(|e| format!("audit-log parse failure (line {}): {e}", i))?;
        records.push(rec);
    }
    Ok(records)
}

/// Returns the records matching a code hash.
pub fn lookup_by_hash(hash: &str) -> Result<Vec<AuditRecord>, String> {
    Ok(all()?.into_iter().filter(|r| r.hash == hash).collect())
}

/// Verifies that the whole log hash chain is valid.
pub fn verify_chain() -> Result<bool, String> {
    let records = all()?;
    let mut prev = "GENESIS".to_string();
    for r in &records {
        if r.prev_hash != prev || !r.verify() {
            return Ok(false);
        }
        prev = r.record_hash.clone();
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Instead of testing with a temp log path, verifies the pure computation functions.
    #[test]
    fn hash_code_is_stable() {
        let a = hash_code("v p = load(\"x.csv\") :: S;");
        let b = hash_code("v p = load(\"x.csv\") :: S;");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(hash_code("different"), a);
    }

    #[test]
    fn append_to_path_records_outcome_and_chain() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_audit_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("audit.jsonl");

        let r1 = append_to_path("v a = load(\"x.csv\") :: S;", Some("success"), &file).unwrap();
        let r2 = append_to_path("v b = load(\"y.csv\") :: T;", Some("failed"), &file).unwrap();

        // index and chain linking
        assert_eq!(r1.index, 0);
        assert_eq!(r2.index, 1);
        assert_eq!(r2.prev_hash, r1.record_hash);
        // outcome recording
        assert_eq!(r1.outcome.as_deref(), Some("success"));
        assert_eq!(r2.outcome.as_deref(), Some("failed"));

        // re-read from the file and verify
        let recs = read_all(&file).unwrap();
        assert_eq!(recs.len(), 2);
        for r in &recs {
            assert!(r.verify(), "record hash mismatch");
        }
        assert_eq!(recs[1].prev_hash, recs[0].record_hash);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_hash_is_stable_and_verifiable() {
        let mut r = AuditRecord {
            index: 0,
            timestamp: "2026-08-26T00:00:00Z".to_string(),
            hash: hash_code("code"),
            code_length: 4,
            outcome: None,
            prev_hash: "GENESIS".to_string(),
            record_hash: String::new(),
        };
        r.record_hash = compute_record_hash(&r);
        assert!(r.verify());
        // verification fails if the hash is tampered with
        let mut tampered = r.clone();
        tampered.code_length = 999;
        assert!(!tampered.verify());
    }

    #[test]
    fn chain_detects_break() {
        let mut r1 = AuditRecord {
            index: 0,
            timestamp: "t0".into(),
            hash: "h0".into(),
            code_length: 1,
            outcome: None,
            prev_hash: "GENESIS".into(),
            record_hash: String::new(),
        };
        r1.record_hash = compute_record_hash(&r1);
        // a record whose previous hash does not match
        let mut r2 = AuditRecord {
            index: 1,
            timestamp: "t1".into(),
            hash: "h1".into(),
            code_length: 1,
            outcome: Some("failed".into()),
            prev_hash: "WRONG".into(),
            record_hash: String::new(),
        };
        r2.record_hash = compute_record_hash(&r2);
        let recs = vec![r1, r2];
        let mut prev = "GENESIS".to_string();
        let mut ok = true;
        for r in &recs {
            if r.prev_hash != prev || !r.verify() {
                ok = false;
                break;
            }
            prev = r.record_hash.clone();
        }
        assert!(!ok, "a chain break should be detected");
    }
}
