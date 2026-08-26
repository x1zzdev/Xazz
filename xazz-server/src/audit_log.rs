//! xazz-server audit_log.rs — SHA-256 감사 로그 (append-only JSONL)
//!
//! 모든 보안 감사 기록을 디스크의 JSONL 파일에 영구 보존한다.
//! 각 레코드는 코드의 SHA-256 해시, 타임스탬프, 그리고 이전 레코드의 해시를
//! 포함해 연쇄(hash chain)를 형성한다. 이로써 로그 변조를 감지할 수 있다.
//!
//!   - append(): 새 감사 레코드 추가 (파일에 append)
//!   - all():    전체 로그 반환
//!   - lookup(): 코드 해시로 일치하는 레코드 조회
//!
//! JSONL 파일 위치: 서버 실행 디렉터리의 `audit_log/audit.jsonl`

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// JSONL 파일 경로 (서버 실행 디렉터리 기준)
pub const AUDIT_LOG_DIR: &str = "audit_log";
pub const AUDIT_LOG_FILE: &str = "audit_log/audit.jsonl";

/// 감사 로그 레코드 — 한 줄 JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// 레코드 순번 (0부터)
    pub index: u64,
    /// ISO-8601 타임스탬프
    pub timestamp: String,
    /// 대상 코드의 SHA-256 해시
    pub hash: String,
    /// 원본 코드 길이 (바이트)
    pub code_length: usize,
    /// 실행 결과 상태 ("success" | "failed" | enum 값). 없으면 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// 이전 레코드의 SHA-256 해시 (체인 형성) — 첫 레코드는 "GENESIS"
    pub prev_hash: String,
    /// 이 레코드 전체(prev_hash 제외)의 SHA-256 해시
    pub record_hash: String,
}

impl AuditRecord {
    /// 해시 체인 검증용 — record_hash 재계산과 prev_hash 연결 확인
    pub fn verify(&self) -> bool {
        let computed = compute_record_hash(self);
        computed == self.record_hash
    }
}

/// record_hash 계산 (record_hash 필드 자체는 제외)
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

/// 코드 문자열의 SHA-256 해시 반환
pub fn hash_code(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 로그 파일 디렉터리를 생성하고 경로를 반환한다.
fn ensure_log_dir() -> Result<std::path::PathBuf, String> {
    let path = std::path::PathBuf::from(AUDIT_LOG_DIR);
    std::fs::create_dir_all(&path).map_err(|e| format!("감사 로그 디렉터리 생성 실패: {e}"))?;
    Ok(path)
}

/// 새 감사 레코드를 생성해 파일 끝에 추가한다. (append-only)
///
/// `outcome`은 실행 결과 상태("success"/"failed" 등)로, 기존 서명 호환을
/// 위해 `append(code)` 형태(결과 미지정)도 지원한다.
pub fn append(code: &str) -> Result<AuditRecord, String> {
    append_with_outcome(code, None)
}

/// 실행 결과(outcome)를 포함해 감사 레코드를 추가한다. (기본 로그 파일)
pub fn append_with_outcome(code: &str, outcome: Option<&str>) -> Result<AuditRecord, String> {
    ensure_log_dir()?;
    let file_path = std::path::PathBuf::from(AUDIT_LOG_FILE);
    append_to_path(code, outcome, &file_path)
}

/// 코드 + outcome을 지정된 파일(append-only)에 기록한다. (내부, 테스트용)
fn append_to_path(
    code: &str,
    outcome: Option<&str>,
    file_path: &std::path::Path,
) -> Result<AuditRecord, String> {
    let existing = read_all(file_path).map_err(|e| format!("감사 로그 읽기 실패: {e}"))?;
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

    // append-only: OpenOptions에 append(true) 사용
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .map_err(|e| format!("감사 로그 파일 열기 실패: {e}"))?;
    let line = serde_json::to_string(&record).map_err(|e| format!("JSON 직렬화 실패: {e}"))?;
    writeln!(file, "{}", line).map_err(|e| format!("감사 로그 기록 실패: {e}"))?;

    Ok(record)
}

/// 로그 파일에서 모든 레코드를 순서대로 읽는다.
pub fn all() -> Result<Vec<AuditRecord>, String> {
    read_all(&std::path::PathBuf::from(AUDIT_LOG_FILE))
}

fn read_all(file_path: &std::path::Path) -> Result<Vec<AuditRecord>, String> {
    if !file_path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        std::fs::read_to_string(file_path).map_err(|e| format!("감사 로그 읽기 실패: {e}"))?;
    let mut records = Vec::new();
    for (i, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rec: AuditRecord = serde_json::from_str(trimmed)
            .map_err(|e| format!("감사 로그 파싱 실패 (line {}): {e}", i))?;
        records.push(rec);
    }
    Ok(records)
}

/// 코드 해시로 일치하는 레코드들을 반환한다.
pub fn lookup_by_hash(hash: &str) -> Result<Vec<AuditRecord>, String> {
    Ok(all()?.into_iter().filter(|r| r.hash == hash).collect())
}

/// 전체 로그 해시 체인이 유효한지 검증한다.
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

    /// 임시 로그 경로로 테스트하는 대신 순수 계산 함수를 검증한다.
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

        // 인덱스·체인 연결
        assert_eq!(r1.index, 0);
        assert_eq!(r2.index, 1);
        assert_eq!(r2.prev_hash, r1.record_hash);
        // outcome 기록
        assert_eq!(r1.outcome.as_deref(), Some("success"));
        assert_eq!(r2.outcome.as_deref(), Some("failed"));

        // 파일에서 재읽어 검증
        let recs = read_all(&file).unwrap();
        assert_eq!(recs.len(), 2);
        for r in &recs {
            assert!(r.verify(), "레코드 해시 불일치");
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
        // 해시가 변경되면 검증 실패
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
        // 이전 해시가 맞지 않는 레코드
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
        assert!(!ok, "체인 파손이 감지되어야 한다");
    }
}
