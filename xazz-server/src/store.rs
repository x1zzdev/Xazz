//! xazz-server/src/store.rs — SQLite run-history persistence (issue C1)
//!
//! Each pipeline execution (`POST /execute`) is recorded in a `runs` table so
//! run history survives a server restart and is queryable via the HTTP API.
//!
//!   GET  /runs        → list runs (id, status, rows, code_hash, created_at)
//!   GET  /runs/:id    → single run record (including stored error)
//!
//! Storage lives in `xazz.db` in the server's working directory. The DB is
//! opened lazily and the table is created if missing.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use serde::Serialize;

/// SQLite database file (relative to the server's working directory).
pub const DB_FILE: &str = "xazz.db";

/// One persisted run record.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: i64,
    pub code_hash: String,
    pub status: String,
    pub rows: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Unix epoch seconds
    pub created_at: i64,
}

/// Holds the lazily-opened SQLite connection.
pub struct Store {
    conn: Mutex<Option<Connection>>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            conn: Mutex::new(None),
        }
    }

    /// Builds a store pinned to an explicit DB path (schema created eagerly).
    /// Used by tests to isolate runs from the real `xazz.db`.
    #[allow(dead_code)]
    pub fn open_at(path: &std::path::Path) -> Self {
        let conn = Connection::open(path).expect("open store db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code_hash TEXT NOT NULL,
                status TEXT NOT NULL,
                rows INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                created_at INTEGER NOT NULL
            );",
        )
        .expect("create runs table");
        Store {
            conn: Mutex::new(Some(conn)),
        }
    }

    /// Opens the DB and creates the schema if needed. Called on first write.
    fn open(&self) -> Result<std::sync::MutexGuard<'_, Option<Connection>>, String> {
        let mut guard = self.conn.lock().map_err(|_| "store lock poisoned".to_string())?;
        if guard.is_none() {
            let conn = Connection::open(PathBuf::from(DB_FILE))
                .map_err(|e| format!("failed to open {}: {e}", DB_FILE))?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    code_hash TEXT NOT NULL,
                    status TEXT NOT NULL,
                    rows INTEGER NOT NULL DEFAULT 0,
                    error TEXT,
                    created_at INTEGER NOT NULL
                );",
            )
            .map_err(|e| format!("failed to create runs table: {e}"))?;
            *guard = Some(conn);
        }
        Ok(guard)
    }

    /// Records a run and returns its id.
    pub fn record_run(
        &self,
        code_hash: &str,
        status: &str,
        rows: i64,
        error: Option<&str>,
    ) -> Result<i64, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        conn.execute(
            "INSERT INTO runs (code_hash, status, rows, error, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![code_hash, status, rows, error, created],
        )
        .map_err(|e| format!("failed to insert run: {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    /// Lists runs newest-first.
    pub fn list_runs(&self, limit: usize) -> Result<Vec<RunRecord>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let mut stmt = conn
            .prepare("SELECT id, code_hash, status, rows, error, created_at FROM runs ORDER BY id DESC LIMIT ?1")
            .map_err(|e| format!("failed to prepare list: {e}"))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(RunRecord {
                    id: row.get(0)?,
                    code_hash: row.get(1)?,
                    status: row.get(2)?,
                    rows: row.get(3)?,
                    error: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("failed to query runs: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("failed to read run row: {e}"))?);
        }
        Ok(out)
    }

    /// Fetches a single run by id.
    pub fn get_run(&self, id: i64) -> Result<Option<RunRecord>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let mut stmt = conn
            .prepare("SELECT id, code_hash, status, rows, error, created_at FROM runs WHERE id = ?1")
            .map_err(|e| format!("failed to prepare get: {e}"))?;
        let mut rows = stmt
            .query_map(params![id], |row| {
                Ok(RunRecord {
                    id: row.get(0)?,
                    code_hash: row.get(1)?,
                    status: row.get(2)?,
                    rows: row.get(3)?,
                    error: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("failed to query run: {e}"))?;
        match rows.next() {
            Some(r) => r.map_err(|e| format!("failed to read run: {e}")).map(Some),
            None => Ok(None),
        }
    }
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_lists_runs() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        let id = store
            .record_run("hash1", "success", 42, None)
            .expect("insert");
        let id2 = store
            .record_run("hash2", "failed", 0, Some("boom"))
            .expect("insert");

        let list = store.list_runs(10).expect("list");
        assert!(list.len() >= 2, "{list:?}");
        let first = &list[0];
        assert!(first.id == id2 || first.id == id);
        // Newest first.
        assert_eq!(first.id.max(id2), first.id);

        let got = store.get_run(id).expect("get").expect("exists");
        assert_eq!(got.status, "success");
        assert_eq!(got.rows, 42);
        assert!(got.error.is_none());

        let got2 = store.get_run(id2).expect("get").expect("exists");
        assert_eq!(got2.status, "failed");
        assert_eq!(got2.error.as_deref(), Some("boom"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_run_returns_none() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_miss_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);
        assert!(store.get_run(999_999).expect("no err").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}