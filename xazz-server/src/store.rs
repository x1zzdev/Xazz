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
//!
//! Multi-tenant (issue C2): every run is tagged with a `tenant`; when a request
//! is authenticated with `X-Xazz-Tenant`, the store only sees/returns that
//! tenant's rows. The `""` tenant is the default (single-tenant / local).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// SQLite database file (relative to the server's working directory).
pub const DB_FILE: &str = "xazz.db";

/// Default cap on retained policy-history rows per tenant (issue C2).
///
/// The table is append-only for audit purposes, but unbounded per-tenant growth
/// is a disk/DoS risk: after each change the oldest rows beyond this cap are
/// dropped. Overridable with `XAZZ_TENANT_POLICY_HISTORY_MAX`.
const DEFAULT_POLICY_HISTORY_MAX: usize = 1000;
/// Environment override for the per-tenant policy-history retention cap.
const POLICY_HISTORY_MAX_ENV: &str = "XAZZ_TENANT_POLICY_HISTORY_MAX";
/// Environment override for the per-tenant policy-history retention window
/// (seconds). Unset or `0` disables time-based expiry, leaving only the count cap.
const POLICY_HISTORY_TTL_ENV: &str = "XAZZ_TENANT_POLICY_HISTORY_TTL_SECS";
/// Default interval between periodic policy-history retention sweeps (seconds).
/// `0` disables the periodic sweep (issue C2).
const DEFAULT_POLICY_HISTORY_SWEEP_SECS: u64 = 3600;
/// Environment override for the periodic policy-history sweep interval.
pub const POLICY_HISTORY_SWEEP_ENV: &str = "XAZZ_POLICY_HISTORY_SWEEP_SECS";

/// Parses the per-tenant policy-history retention cap (issue C2).
///
/// Invalid or `0` values fall back to [`DEFAULT_POLICY_HISTORY_MAX`] so the cap
/// can never be disabled accidentally.
pub fn resolve_policy_history_max(raw: Option<&str>) -> usize {
    raw.and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_POLICY_HISTORY_MAX)
}

/// Parses the per-tenant policy-history retention window in seconds (issue C2).
///
/// `0` (the default) disables time-based expiry. Invalid values also fall back to
/// `0` rather than a shorter window that could silently discard audit rows; the
/// count cap ([`resolve_policy_history_max`]) still bounds growth.
pub fn resolve_policy_history_ttl(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

/// Parses the periodic policy-history sweep interval in seconds (issue C2).
///
/// `0` disables the periodic sweep. Invalid values fall back to
/// [`DEFAULT_POLICY_HISTORY_SWEEP_SECS`] so a typo cannot silently stop cleaning.
pub fn resolve_policy_history_sweep(raw: Option<&str>) -> u64 {
    match raw {
        None => DEFAULT_POLICY_HISTORY_SWEEP_SECS,
        Some(v) => v
            .parse::<u64>()
            .unwrap_or(DEFAULT_POLICY_HISTORY_SWEEP_SECS),
    }
}

/// Current Unix epoch seconds.
fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Creates the run-history and per-tenant DP-budget tables if missing (idempotent).
fn ensure_schema(conn: &Connection) -> Result<(), String> {
    // Multi-instance deployments share one SQLite file: without a busy timeout a
    // concurrent writer fails immediately with SQLITE_BUSY instead of waiting for
    // the other instance's short transaction (issue C2).
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("failed to set sqlite busy timeout: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            code_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            rows INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            created_at INTEGER NOT NULL,
            tenant TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS dp_budget (
            tenant TEXT PRIMARY KEY,
            spent_epsilon REAL NOT NULL DEFAULT 0,
            spent_delta REAL NOT NULL DEFAULT 0,
            window_started_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS tenant_policies (
            tenant TEXT PRIMARY KEY,
            policy_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS tenant_dp_config (
            tenant TEXT PRIMARY KEY,
            window_secs INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS tenant_policy_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tenant TEXT NOT NULL,
            action TEXT NOT NULL,
            old_policy_json TEXT,
            new_policy_json TEXT,
            changed_by TEXT NOT NULL DEFAULT '',
            changed_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS tenant_policy_history_config (
            tenant TEXT PRIMARY KEY,
            ttl_secs INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS tenant_policy_history_config_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tenant TEXT NOT NULL,
            action TEXT NOT NULL,
            old_ttl_secs INTEGER,
            new_ttl_secs INTEGER,
            changed_by TEXT NOT NULL DEFAULT '',
            changed_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS dp_reservation (
            tenant TEXT PRIMARY KEY,
            reservation_id TEXT NOT NULL,
            reserved_epsilon REAL NOT NULL DEFAULT 0,
            reserved_delta REAL NOT NULL DEFAULT 0,
            reserved_at INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER NOT NULL DEFAULT 0
        );",
    )
    .map_err(|e| format!("failed to create store schema: {e}"))?;
    // Migration for DBs created before the tenant column (issue C2):
    // adding an already-present column is a no-op error we swallow.
    let _ = conn.execute_batch("ALTER TABLE runs ADD COLUMN tenant TEXT NOT NULL DEFAULT ''");
    // Migration for DBs created before budget windows (issue C2):
    let _ = conn.execute_batch(
        "ALTER TABLE dp_budget ADD COLUMN window_started_at INTEGER NOT NULL DEFAULT 0",
    );
    Ok(())
}

/// One persisted run record.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: i64,
    pub code_hash: String,
    pub status: String,
    pub rows: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Tenant tag (empty string = default/single-tenant) — issue C2
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tenant: String,
    /// Unix epoch seconds
    pub created_at: i64,
}

/// One append-only policy-pack change record (issue C2).
///
/// `action` is `"set"` (create/replace) or `"delete"`. `old_policy_json` is the
/// pack that was in effect before the change (absent for the first set), and
/// `new_policy_json` is the pack after it (absent for a delete). `changed_by`
/// is the authenticated tenant that performed the self-service change.
#[derive(Debug, Clone, Serialize)]
pub struct PolicyChangeRecord {
    pub id: i64,
    pub tenant: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_policy_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_policy_json: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub changed_by: String,
    /// Unix epoch seconds
    pub changed_at: i64,
}

/// One append-only record of a per-tenant policy-history retention-window
/// override change (issue C2).
///
/// `action` is `"set"` (create/replace the override) or `"clear"` (remove it,
/// restoring the global default). `old_ttl_secs` is the override in effect before
/// the change (absent for the first set), and `new_ttl_secs` is the override after
/// it (absent for a clear). `changed_by` is the authenticated tenant that
/// performed the self-service change.
#[derive(Debug, Clone, Serialize)]
pub struct PolicyHistoryTtlChangeRecord {
    pub id: i64,
    pub tenant: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_ttl_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_ttl_secs: Option<u64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub changed_by: String,
    /// Unix epoch seconds
    pub changed_at: i64,
}

/// A reserved slice of a tenant's DP envelope for one in-flight run — issue C2.
///
/// The reservation reserves the tenant's *entire remaining* envelope so that a
/// second server instance cannot hand the same budget to a concurrent run.
/// `id` is an opaque token the holder passes back to
/// [`Store::settle_dp_reservation`]; if the reservation expired and was
/// reclaimed by another instance, the stale token matches nothing and the
/// reclaiming reservation is left intact.
#[derive(Debug, Clone, Serialize)]
pub struct DpReservation {
    pub id: String,
    pub epsilon: f64,
    pub delta: f64,
}

/// Holds the lazily-opened SQLite connection.
pub struct Store {
    conn: Mutex<Option<Connection>>,
    /// Max policy-history rows retained per tenant (issue C2).
    history_max: usize,
    /// Global policy-history retention window in seconds (`0` = no time-based
    /// expiry). A stored per-tenant override (`tenant_policy_history_config`)
    /// takes precedence over this default.
    history_ttl_secs: u64,
}

impl Store {
    pub fn new() -> Self {
        let history_max =
            resolve_policy_history_max(std::env::var(POLICY_HISTORY_MAX_ENV).ok().as_deref());
        let history_ttl_secs =
            resolve_policy_history_ttl(std::env::var(POLICY_HISTORY_TTL_ENV).ok().as_deref());
        Store {
            conn: Mutex::new(None),
            history_max,
            history_ttl_secs,
        }
    }

    /// Builds a store pinned to an explicit DB path (schema created eagerly).
    /// Used by tests to isolate runs from the real `xazz.db`.
    #[allow(dead_code)]
    pub fn open_at(path: &std::path::Path) -> Self {
        Self::open_at_with_history(path, DEFAULT_POLICY_HISTORY_MAX, 0)
    }

    /// Like [`Store::open_at`] but with an explicit retention cap (tests).
    #[allow(dead_code)]
    fn open_at_with_history_max(path: &std::path::Path, history_max: usize) -> Self {
        Self::open_at_with_history(path, history_max, 0)
    }

    /// Like [`Store::open_at`] but with explicit retention settings (tests).
    fn open_at_with_history(
        path: &std::path::Path,
        history_max: usize,
        history_ttl_secs: u64,
    ) -> Self {
        let conn = Connection::open(path).expect("open store db");
        ensure_schema(&conn).expect("create store schema");
        Store {
            conn: Mutex::new(Some(conn)),
            history_max,
            history_ttl_secs,
        }
    }

    /// Opens the DB and creates the schema if needed. Called on first write.
    fn open(&self) -> Result<std::sync::MutexGuard<'_, Option<Connection>>, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        if guard.is_none() {
            let conn = Connection::open(PathBuf::from(DB_FILE))
                .map_err(|e| format!("failed to open {}: {e}", DB_FILE))?;
            ensure_schema(&conn)?;
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
        tenant: &str,
    ) -> Result<i64, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let created = now_epoch();
        conn.execute(
            "INSERT INTO runs (code_hash, status, rows, error, created_at, tenant) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![code_hash, status, rows, error, created, tenant],
        )
        .map_err(|e| format!("failed to insert run: {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    /// Lists runs newest-first, filtered to a tenant.
    pub fn list_runs(&self, limit: usize, tenant: &str) -> Result<Vec<RunRecord>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let mut stmt = conn
            .prepare("SELECT id, code_hash, status, rows, error, created_at, tenant FROM runs WHERE tenant = ?1 ORDER BY id DESC LIMIT ?2")
            .map_err(|e| format!("failed to prepare list: {e}"))?;
        let rows = stmt
            .query_map(params![tenant, limit as i64], |row| {
                Ok(RunRecord {
                    id: row.get(0)?,
                    code_hash: row.get(1)?,
                    status: row.get(2)?,
                    rows: row.get(3)?,
                    error: row.get(4)?,
                    created_at: row.get(5)?,
                    tenant: row.get(6)?,
                })
            })
            .map_err(|e| format!("failed to query runs: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("failed to read run row: {e}"))?);
        }
        Ok(out)
    }

    /// Fetches a single run by id, scoped to a tenant.
    pub fn get_run(&self, id: i64, tenant: &str) -> Result<Option<RunRecord>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let mut stmt = conn
            .prepare("SELECT id, code_hash, status, rows, error, created_at, tenant FROM runs WHERE id = ?1 AND tenant = ?2")
            .map_err(|e| format!("failed to prepare get: {e}"))?;
        let mut rows = stmt
            .query_map(params![id, tenant], |row| {
                Ok(RunRecord {
                    id: row.get(0)?,
                    code_hash: row.get(1)?,
                    status: row.get(2)?,
                    rows: row.get(3)?,
                    error: row.get(4)?,
                    created_at: row.get(5)?,
                    tenant: row.get(6)?,
                })
            })
            .map_err(|e| format!("failed to query run: {e}"))?;
        match rows.next() {
            Some(r) => r.map_err(|e| format!("failed to read run: {e}")).map(Some),
            None => Ok(None),
        }
    }

    /// Returns the tenant's cumulative DP spend as `(epsilon, delta)` — issue C2.
    ///
    /// Unknown tenants report `(0.0, 0.0)` (no budget consumed yet). The ledger is
    /// keyed by tenant, so one tenant's spend never affects another's.
    ///
    /// When `window_secs > 0` the budget is a *sliding window*: once the window
    /// anchored at `window_started_at` has elapsed, the spend is reset to zero
    /// (the roll is persisted so later accruals start a fresh window).
    /// `window_secs == 0` keeps the legacy cumulative behavior.
    pub fn dp_spent(&self, tenant: &str, window_secs: u64) -> Result<(f64, f64), String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        roll_dp_window(conn, tenant, window_secs, now_epoch())?;
        conn.query_row(
            "SELECT spent_epsilon, spent_delta FROM dp_budget WHERE tenant = ?1",
            params![tenant],
            |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
        )
        .optional()
        .map(|opt| opt.unwrap_or((0.0, 0.0)))
        .map_err(|e| format!("failed to read dp budget: {e}"))
    }

    /// Returns the tenant's current window anchor (Unix epoch seconds), if a row exists.
    pub fn dp_window_started_at(&self, tenant: &str) -> Result<Option<i64>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        conn.query_row(
            "SELECT window_started_at FROM dp_budget WHERE tenant = ?1",
            params![tenant],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| format!("failed to read dp window: {e}"))
    }

    /// Returns the tenant's live (non-expired) DP reservation, if any — issue #123.
    ///
    /// A reservation holds the tenant's remaining envelope while a run is in
    /// flight. The budget view subtracts it so `remaining_*` reflects what a new
    /// run could actually claim right now. Expired reservations are ignored; they
    /// are reclaimable and no longer hold the budget.
    pub fn live_dp_reservation(
        &self,
        tenant: &str,
    ) -> Result<Option<(DpReservation, i64)>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        conn.query_row(
            "SELECT reservation_id, reserved_epsilon, reserved_delta, expires_at
             FROM dp_reservation WHERE tenant = ?1 AND expires_at > ?2",
            params![tenant, now_epoch()],
            |row| {
                Ok((
                    DpReservation {
                        id: row.get(0)?,
                        epsilon: row.get(1)?,
                        delta: row.get(2)?,
                    },
                    row.get(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("failed to read dp reservation: {e}"))
    }

    /// Adds a run's DP spend to the tenant's cumulative ledger — issue C2.
    ///
    /// The increment is a single atomic UPSERT, so two concurrent runs of the same
    /// tenant cannot lose a spend. Non-positive spends are a no-op. Non-finite
    /// values are rejected so a malformed marker cannot poison the ledger.
    ///
    /// `window_secs` behaves exactly as in [`Store::dp_spent`]: an elapsed window is
    /// rolled (zeroed) before the increment is applied.
    ///
    /// Retained as the direct accrual primitive; `/execute` now bills through
    /// [`Store::settle_dp_reservation`] so the accrual and reservation release are
    /// atomic. Tests use it to seed ledger state.
    #[allow(dead_code)]
    pub fn add_dp_spend(
        &self,
        tenant: &str,
        epsilon: f64,
        delta: f64,
        window_secs: u64,
    ) -> Result<(), String> {
        if !epsilon.is_finite() || !delta.is_finite() {
            return Err("dp spend must be finite".to_string());
        }
        let epsilon = epsilon.max(0.0);
        let delta = delta.max(0.0);
        if epsilon == 0.0 && delta == 0.0 {
            return Ok(());
        }
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let now = now_epoch();
        roll_dp_window(conn, tenant, window_secs, now)?;
        conn.execute(
            "INSERT INTO dp_budget (tenant, spent_epsilon, spent_delta, window_started_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(tenant) DO UPDATE SET
                 spent_epsilon = spent_epsilon + excluded.spent_epsilon,
                 spent_delta   = spent_delta   + excluded.spent_delta,
                 updated_at    = excluded.updated_at",
            params![tenant, epsilon, delta, now],
        )
        .map_err(|e| format!("failed to update dp budget: {e}"))?;
        Ok(())
    }

    /// Atomically reserves a tenant's entire remaining DP envelope for one run — issue C2.
    ///
    /// The in-process per-tenant lock only serializes runs inside one server
    /// process. Across instances the reservation is the cross-process lock: the
    /// reserve is a single SQLite statement that reads the ledger and writes the
    /// reservation together, so exactly one instance can hold a tenant's
    /// reservation at a time. A second instance gets `Ok(None)` and must
    /// fail-closed instead of handing the same budget to another run.
    ///
    /// An elapsed window is rolled before the remaining envelope is computed, and
    /// a reservation whose `ttl_secs` has passed is reclaimed in the same
    /// statement (so an instance that crashed mid-run cannot block the tenant
    /// forever). Only the tenant's own reservation is considered; other tenants
    /// are unaffected.
    ///
    /// Returns `Some(reservation)` with the reserved (remaining) ε/δ, or `None`
    /// when another live reservation already holds the tenant.
    pub fn reserve_dp_budget(
        &self,
        tenant: &str,
        total_epsilon: f64,
        total_delta: f64,
        window_secs: u64,
        ttl_secs: u64,
    ) -> Result<Option<DpReservation>, String> {
        if !total_epsilon.is_finite() || !total_delta.is_finite() {
            return Err("dp envelope must be finite".to_string());
        }
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let now = now_epoch();
        // Roll an elapsed window first so "remaining" is computed against the
        // current window. The reserve statement below is what serializes writers.
        roll_dp_window(conn, tenant, window_secs, now)?;

        let reservation_id = uuid::Uuid::new_v4().to_string();
        let expires_at = now + ttl_secs.max(1) as i64;
        // A single statement: insert the reservation only when no live one exists,
        // computing the remaining envelope from the ledger. If an expired row is
        // present the conflict clause overwrites it (that is the reclaim path).
        let changed = conn
            .execute(
                "INSERT INTO dp_reservation
                     (tenant, reservation_id, reserved_epsilon, reserved_delta, reserved_at, expires_at)
                 SELECT ?1, ?2,
                        MAX(?3 - COALESCE((SELECT spent_epsilon FROM dp_budget WHERE tenant = ?1), 0.0), 0.0),
                        MAX(?4 - COALESCE((SELECT spent_delta   FROM dp_budget WHERE tenant = ?1), 0.0), 0.0),
                        ?5, ?6
                 WHERE NOT EXISTS (
                     SELECT 1 FROM dp_reservation WHERE tenant = ?1 AND expires_at > ?5
                 )
                 ON CONFLICT(tenant) DO UPDATE SET
                     reservation_id   = excluded.reservation_id,
                     reserved_epsilon = excluded.reserved_epsilon,
                     reserved_delta   = excluded.reserved_delta,
                     reserved_at      = excluded.reserved_at,
                     expires_at       = excluded.expires_at
                 WHERE dp_reservation.expires_at <= ?5",
                params![tenant, reservation_id, total_epsilon, total_delta, now, expires_at],
            )
            .map_err(|e| format!("failed to reserve dp budget: {e}"))?;

        if changed == 0 {
            return Ok(None);
        }
        conn.query_row(
            "SELECT reservation_id, reserved_epsilon, reserved_delta
             FROM dp_reservation WHERE tenant = ?1",
            params![tenant],
            |row| {
                Ok(DpReservation {
                    id: row.get(0)?,
                    epsilon: row.get(1)?,
                    delta: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| format!("failed to read dp reservation: {e}"))
    }

    /// Releases a reservation and bills the run's *actual* spend in one transaction — issue C2.
    ///
    /// Only the reservation with the matching `reservation_id` is removed, so a
    /// stale settle (its reservation was reclaimed after the TTL expired) cannot
    /// release a newer holder's reservation. `epsilon`/`delta` are the spend the
    /// run actually consumed (0 for a run with no `withDp`), and an elapsed window
    /// is rolled before it is billed.
    pub fn settle_dp_reservation(
        &self,
        tenant: &str,
        reservation_id: &str,
        epsilon: f64,
        delta: f64,
        window_secs: u64,
    ) -> Result<(), String> {
        if !epsilon.is_finite() || !delta.is_finite() {
            return Err("dp spend must be finite".to_string());
        }
        let epsilon = epsilon.max(0.0);
        let delta = delta.max(0.0);
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin dp settle transaction: {e}"))?;
        let now = now_epoch();
        roll_dp_window(&tx, tenant, window_secs, now)?;
        if epsilon != 0.0 || delta != 0.0 {
            tx.execute(
                "INSERT INTO dp_budget (tenant, spent_epsilon, spent_delta, window_started_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(tenant) DO UPDATE SET
                     spent_epsilon = spent_epsilon + excluded.spent_epsilon,
                     spent_delta   = spent_delta   + excluded.spent_delta,
                     updated_at    = excluded.updated_at",
                params![tenant, epsilon, delta, now],
            )
            .map_err(|e| format!("failed to update dp budget: {e}"))?;
        }
        tx.execute(
            "DELETE FROM dp_reservation WHERE tenant = ?1 AND reservation_id = ?2",
            params![tenant, reservation_id],
        )
        .map_err(|e| format!("failed to release dp reservation: {e}"))?;
        tx.commit()
            .map_err(|e| format!("failed to commit dp settle transaction: {e}"))?;
        Ok(())
    }

    /// Resets a tenant's accumulated DP spend to zero and restarts its window — issue C2.
    ///
    /// Tenant-scoped: other tenants' ledgers are untouched. A zeroed row is kept
    /// (rather than deleted) and re-anchored to now so a fresh window starts immediately.
    pub fn reset_dp_budget(&self, tenant: &str) -> Result<(), String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let now = now_epoch();
        conn.execute(
            "INSERT INTO dp_budget (tenant, spent_epsilon, spent_delta, window_started_at, updated_at)
             VALUES (?1, 0, 0, ?2, ?2)
             ON CONFLICT(tenant) DO UPDATE SET
                 spent_epsilon     = 0,
                 spent_delta       = 0,
                 window_started_at = ?2,
                 updated_at        = ?2",
            params![tenant, now],
        )
        .map_err(|e| format!("failed to reset dp budget: {e}"))?;
        Ok(())
    }

    /// Returns the tenant's stored DP window override in seconds, if any — issue C2.
    ///
    /// `None` means the tenant has no override and the global default applies.
    /// `Some(0)` is an explicit per-tenant "cumulative, no window" setting and must
    /// not be confused with the absence of an override.
    pub fn get_dp_window(&self, tenant: &str) -> Result<Option<u64>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        conn.query_row(
            "SELECT window_secs FROM tenant_dp_config WHERE tenant = ?1",
            params![tenant],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|opt| opt.map(|secs| secs.max(0) as u64))
        .map_err(|e| format!("failed to read tenant dp window: {e}"))
    }

    /// Stores a tenant's DP window override — issue C2.
    ///
    /// The window is keyed by tenant, so one tenant's setting never changes
    /// another's. If the tenant already has a budget row, its window anchor is
    /// re-anchored to now so the newly configured window starts at set time
    /// (changing the length cannot retroactively expire spend that was accrued
    /// under the previous setting).
    pub fn set_dp_window(&self, tenant: &str, window_secs: u64) -> Result<(), String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin dp window transaction: {e}"))?;
        let now = now_epoch();
        tx.execute(
            "INSERT INTO tenant_dp_config (tenant, window_secs, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(tenant) DO UPDATE SET
                 window_secs = excluded.window_secs,
                 updated_at  = excluded.updated_at",
            params![tenant, window_secs as i64, now],
        )
        .map_err(|e| format!("failed to store tenant dp window: {e}"))?;
        tx.execute(
            "UPDATE dp_budget SET window_started_at = ?1, updated_at = ?1 WHERE tenant = ?2",
            params![now, tenant],
        )
        .map_err(|e| format!("failed to re-anchor dp window: {e}"))?;
        tx.commit()
            .map_err(|e| format!("failed to commit dp window transaction: {e}"))?;
        Ok(())
    }

    /// Removes a tenant's DP window override. Returns `true` if one existed — issue C2.
    ///
    /// After removal the tenant falls back to the global `XAZZ_TENANT_DP_WINDOW_SECS`
    /// default. Other tenants' overrides are untouched.
    pub fn clear_dp_window(&self, tenant: &str) -> Result<bool, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let deleted = conn
            .execute(
                "DELETE FROM tenant_dp_config WHERE tenant = ?1",
                params![tenant],
            )
            .map_err(|e| format!("failed to clear tenant dp window: {e}"))?;
        Ok(deleted > 0)
    }

    /// The global policy-history retention window resolved at construction — issue C2.
    ///
    /// This is the fallback for tenants without a stored override.
    pub fn policy_history_ttl_default(&self) -> u64 {
        self.history_ttl_secs
    }

    /// Returns the tenant's stored policy-history retention-window override in
    /// seconds, if any — issue C2.
    ///
    /// `None` means the tenant has no override and the global default applies.
    /// `Some(0)` is an explicit per-tenant "no time-based expiry" setting and must
    /// not be confused with the absence of an override.
    pub fn get_policy_history_ttl(&self, tenant: &str) -> Result<Option<u64>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        read_policy_history_ttl(conn, tenant)
    }

    /// Stores a tenant's policy-history retention-window override and appends an
    /// append-only change record — issue C2.
    ///
    /// The window is keyed by tenant, so one tenant's setting never changes
    /// another's. The previous override (if any) is captured in the same
    /// transaction, so the audit trail is never out of sync with the stored
    /// override. `changed_by` is the authenticated tenant performing the
    /// self-service change.
    pub fn set_policy_history_ttl(
        &self,
        tenant: &str,
        ttl_secs: u64,
        changed_by: &str,
    ) -> Result<(), String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin policy history ttl transaction: {e}"))?;
        let now = now_epoch();
        let previous = read_policy_history_ttl(&tx, tenant)?;
        tx.execute(
            "INSERT INTO tenant_policy_history_config (tenant, ttl_secs, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(tenant) DO UPDATE SET
                 ttl_secs   = excluded.ttl_secs,
                 updated_at = excluded.updated_at",
            params![tenant, ttl_secs as i64, now],
        )
        .map_err(|e| format!("failed to store tenant policy history ttl: {e}"))?;
        insert_policy_history_ttl_change(
            &tx,
            tenant,
            "set",
            previous,
            Some(ttl_secs),
            changed_by,
            now,
        )?;
        prune_policy_history_ttl_history(&tx, tenant, self.history_max)?;
        tx.commit()
            .map_err(|e| format!("failed to commit policy history ttl transaction: {e}"))?;
        Ok(())
    }

    /// Removes a tenant's policy-history retention-window override and appends an
    /// append-only change record. Returns `true` if one existed — issue C2.
    ///
    /// After removal the tenant falls back to the global
    /// `XAZZ_TENANT_POLICY_HISTORY_TTL_SECS` default. Other tenants' overrides are
    /// untouched. The removal and its change record commit together; a clear that
    /// removes nothing records nothing.
    pub fn clear_policy_history_ttl(&self, tenant: &str, changed_by: &str) -> Result<bool, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin policy history ttl transaction: {e}"))?;
        let now = now_epoch();
        let previous = read_policy_history_ttl(&tx, tenant)?;
        let deleted = tx
            .execute(
                "DELETE FROM tenant_policy_history_config WHERE tenant = ?1",
                params![tenant],
            )
            .map_err(|e| format!("failed to clear tenant policy history ttl: {e}"))?;
        if deleted > 0 {
            insert_policy_history_ttl_change(
                &tx, tenant, "clear", previous, None, changed_by, now,
            )?;
            prune_policy_history_ttl_history(&tx, tenant, self.history_max)?;
        }
        tx.commit()
            .map_err(|e| format!("failed to commit policy history ttl transaction: {e}"))?;
        Ok(deleted > 0)
    }

    /// Lists a tenant's policy-history retention-window override change history,
    /// newest-first — issue C2.
    ///
    /// History is tenant-scoped like the overrides themselves, so one tenant's
    /// change trail is never visible to another. The rows are append-only up to
    /// the retention cap (see [`resolve_policy_history_max`]) and survive
    /// override replacement/removal. `limit`/`offset` page the newest-first list.
    pub fn list_policy_history_ttl_history(
        &self,
        tenant: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PolicyHistoryTtlChangeRecord>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let mut stmt = conn
            .prepare(
                "SELECT id, tenant, action, old_ttl_secs, new_ttl_secs, changed_by, changed_at
                 FROM tenant_policy_history_config_history
                 WHERE tenant = ?1
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| format!("failed to prepare policy history ttl history: {e}"))?;
        let rows = stmt
            .query_map(params![tenant, limit as i64, offset as i64], |row| {
                Ok(PolicyHistoryTtlChangeRecord {
                    id: row.get(0)?,
                    tenant: row.get(1)?,
                    action: row.get(2)?,
                    old_ttl_secs: row.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
                    new_ttl_secs: row.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u64),
                    changed_by: row.get(5)?,
                    changed_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("failed to query policy history ttl history: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("failed to read policy history ttl history row: {e}"))?);
        }
        Ok(out)
    }

    /// Returns the tenant's stored policy pack JSON, if any — issue C2.
    ///
    /// The pack is keyed by tenant, so one tenant's policy pack is never applied to
    /// another. The caller is responsible for parsing/validating it (fail-closed).
    pub fn get_tenant_policy(&self, tenant: &str) -> Result<Option<String>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        conn.query_row(
            "SELECT policy_json FROM tenant_policies WHERE tenant = ?1",
            params![tenant],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("failed to read tenant policy: {e}"))
    }

    /// Stores (or replaces) a tenant's policy pack JSON and appends an
    /// append-only change record — issue C2.
    ///
    /// Validation happens before the write (the caller parses with
    /// `Policy::from_json_str`), so a stored pack is always well-formed.
    /// `changed_by` is the authenticated tenant performing the self-service
    /// change; the previous pack (if any) is captured in the same transaction so
    /// the audit trail is never out of sync with the stored pack.
    pub fn set_tenant_policy(
        &self,
        tenant: &str,
        policy_json: &str,
        changed_by: &str,
    ) -> Result<(), String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin policy transaction: {e}"))?;
        let now = now_epoch();
        let previous = read_policy_json(&tx, tenant)?;
        tx.execute(
            "INSERT INTO tenant_policies (tenant, policy_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(tenant) DO UPDATE SET
                 policy_json = excluded.policy_json,
                 updated_at  = excluded.updated_at",
            params![tenant, policy_json, now],
        )
        .map_err(|e| format!("failed to store tenant policy: {e}"))?;
        insert_policy_change(
            &tx,
            tenant,
            "set",
            previous.as_deref(),
            Some(policy_json),
            changed_by,
            now,
        )?;
        prune_policy_history(
            &tx,
            tenant,
            self.history_max,
            effective_history_ttl(&tx, tenant, self.history_ttl_secs)?,
            now,
        )?;
        tx.commit()
            .map_err(|e| format!("failed to commit policy transaction: {e}"))?;
        Ok(())
    }

    /// Deletes a tenant's stored policy pack. Returns `true` if one existed — issue C2.
    ///
    /// The removal and its append-only change record (`action = "delete"`) commit
    /// together; the captured `old_policy_json` preserves what was in effect.
    pub fn delete_tenant_policy(&self, tenant: &str, changed_by: &str) -> Result<bool, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin policy transaction: {e}"))?;
        let now = now_epoch();
        let previous = read_policy_json(&tx, tenant)?;
        let deleted = tx
            .execute(
                "DELETE FROM tenant_policies WHERE tenant = ?1",
                params![tenant],
            )
            .map_err(|e| format!("failed to delete tenant policy: {e}"))?;
        if deleted > 0 {
            insert_policy_change(
                &tx,
                tenant,
                "delete",
                previous.as_deref(),
                None,
                changed_by,
                now,
            )?;
            prune_policy_history(
                &tx,
                tenant,
                self.history_max,
                effective_history_ttl(&tx, tenant, self.history_ttl_secs)?,
                now,
            )?;
        }
        tx.commit()
            .map_err(|e| format!("failed to commit policy transaction: {e}"))?;
        Ok(deleted > 0)
    }

    /// Lists a tenant's policy-pack change history, newest-first — issue C2.
    ///
    /// History is tenant-scoped like the packs themselves, so one tenant's change
    /// trail is never visible to another. The rows are append-only up to the
    /// retention cap (see [`resolve_policy_history_max`]) and retention window —
    /// the tenant's stored override when present, else the global
    /// (see [`resolve_policy_history_ttl`]) — and survive pack
    /// replacement/deletion. `limit`/`offset` page the newest-first list; rows
    /// older than the retention window are filtered out even if a write has not
    /// pruned them yet.
    pub fn list_policy_history(
        &self,
        tenant: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PolicyChangeRecord>, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let ttl = effective_history_ttl(conn, tenant, self.history_ttl_secs)?;
        let cutoff = history_cutoff(ttl, now_epoch());
        let mut stmt = conn
            .prepare(
                "SELECT id, tenant, action, old_policy_json, new_policy_json, changed_by, changed_at
                 FROM tenant_policy_history
                 WHERE tenant = ?1 AND changed_at >= ?2
                 ORDER BY id DESC LIMIT ?3 OFFSET ?4",
            )
            .map_err(|e| format!("failed to prepare policy history: {e}"))?;
        let rows = stmt
            .query_map(
                params![tenant, cutoff, limit as i64, offset as i64],
                |row| {
                    Ok(PolicyChangeRecord {
                        id: row.get(0)?,
                        tenant: row.get(1)?,
                        action: row.get(2)?,
                        old_policy_json: row.get(3)?,
                        new_policy_json: row.get(4)?,
                        changed_by: row.get(5)?,
                        changed_at: row.get(6)?,
                    })
                },
            )
            .map_err(|e| format!("failed to query policy history: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("failed to read policy history row: {e}"))?);
        }
        Ok(out)
    }

    /// Physically trims policy-history rows for every tenant — periodic sweep
    /// (issue C2).
    ///
    /// Pruning normally runs inside a pack/override change, so an idle tenant
    /// whose retention window elapsed (or whose count cap was lowered) keeps
    /// stale rows on disk (hidden only by the read filter). This sweep enforces
    /// both retentions without waiting for a write: it removes rows older than the
    /// tenant's effective window (stored override first, else the global default,
    /// so an explicit `0` override is never expired) and trims both the policy
    /// history and the retention-window override change history to the count cap
    /// ([`resolve_policy_history_max`]). The override change history only ever
    /// gets the count cap — its own window must not expire the audit trail of who
    /// set it. Tenants are discovered from both tables so an override-only tenant
    /// is still trimmed. Returns the number of rows removed.
    pub fn sweep_expired_policy_history(&self) -> Result<usize, String> {
        let guard = self.open()?;
        let conn = guard.as_ref().expect("open guarantees Some");
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin policy sweep transaction: {e}"))?;
        let now = now_epoch();
        let tenants = {
            let mut stmt = tx
                .prepare(
                    "SELECT tenant FROM tenant_policy_history
                     UNION
                     SELECT tenant FROM tenant_policy_history_config_history",
                )
                .map_err(|e| format!("failed to list policy-history tenants: {e}"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| format!("failed to query policy-history tenants: {e}"))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(|e| format!("failed to read policy-history tenant: {e}"))?);
            }
            out
        };
        let mut removed = 0usize;
        for tenant in tenants {
            let ttl = effective_history_ttl(&tx, &tenant, self.history_ttl_secs)?;
            removed += prune_policy_history(&tx, &tenant, self.history_max, ttl, now)?;
            removed += prune_policy_history_ttl_history(&tx, &tenant, self.history_max)?;
        }
        tx.commit()
            .map_err(|e| format!("failed to commit policy sweep: {e}"))?;
        Ok(removed)
    }
}

/// Oldest `changed_at` still retained under a retention window (`0` = no limit).
///
/// Uses `0` as the "no limit" cutoff: `changed_at` is always a positive Unix
/// epoch, so `changed_at >= 0` keeps every row.
fn history_cutoff(ttl_secs: u64, now: i64) -> i64 {
    if ttl_secs > 0 {
        now - ttl_secs as i64
    } else {
        0
    }
}

/// Reads a tenant's stored policy-history retention-window override, if any.
fn read_policy_history_ttl(conn: &Connection, tenant: &str) -> Result<Option<u64>, String> {
    conn.query_row(
        "SELECT ttl_secs FROM tenant_policy_history_config WHERE tenant = ?1",
        params![tenant],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|opt| opt.map(|secs| secs.max(0) as u64))
    .map_err(|e| format!("failed to read tenant policy history ttl: {e}"))
}

/// Resolves a tenant's effective policy-history retention window.
///
/// A stored per-tenant override takes precedence over the global `default_ttl`.
/// Called with the connection bound to the surrounding write transaction so the
/// retained rows and the change that triggers pruning stay consistent.
fn effective_history_ttl(conn: &Connection, tenant: &str, default_ttl: u64) -> Result<u64, String> {
    Ok(read_policy_history_ttl(conn, tenant)?.unwrap_or(default_ttl))
}

/// Reads a tenant's stored pack within a transaction, if present.
fn read_policy_json(conn: &Connection, tenant: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT policy_json FROM tenant_policies WHERE tenant = ?1",
        params![tenant],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| format!("failed to read tenant policy: {e}"))
}

/// Appends one policy-pack change row (never updates/deletes).
fn insert_policy_change(
    conn: &Connection,
    tenant: &str,
    action: &str,
    old_policy_json: Option<&str>,
    new_policy_json: Option<&str>,
    changed_by: &str,
    changed_at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO tenant_policy_history
             (tenant, action, old_policy_json, new_policy_json, changed_by, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            tenant,
            action,
            old_policy_json,
            new_policy_json,
            changed_by,
            changed_at
        ],
    )
    .map_err(|e| format!("failed to record policy change: {e}"))?;
    Ok(())
}

/// Trims a tenant's policy history to its retention cap and window (issue C2).
///
/// Runs in the same transaction as the change that triggered it, so the table
/// never grows unbounded while the newest rows (the ones the audit endpoint
/// serves) are always preserved. Rows older than the retention window are
/// removed first; the count cap then keeps at most `max` rows. Other tenants are
/// never touched. Returns the number of rows removed.
fn prune_policy_history(
    conn: &Connection,
    tenant: &str,
    max: usize,
    ttl_secs: u64,
    now: i64,
) -> Result<usize, String> {
    let mut removed = 0usize;
    if ttl_secs > 0 {
        removed += conn
            .execute(
                "DELETE FROM tenant_policy_history WHERE tenant = ?1 AND changed_at < ?2",
                params![tenant, history_cutoff(ttl_secs, now)],
            )
            .map_err(|e| format!("failed to expire policy history: {e}"))?;
    }
    if max == 0 {
        return Ok(removed);
    }
    removed += conn
        .execute(
            "DELETE FROM tenant_policy_history
         WHERE tenant = ?1
           AND id NOT IN (
               SELECT id FROM tenant_policy_history
               WHERE tenant = ?1 ORDER BY id DESC LIMIT ?2
           )",
            params![tenant, max as i64],
        )
        .map_err(|e| format!("failed to prune policy history: {e}"))?;
    Ok(removed)
}

/// Appends one policy-history retention-window override change row (never
/// updates/deletes) — issue C2.
fn insert_policy_history_ttl_change(
    conn: &Connection,
    tenant: &str,
    action: &str,
    old_ttl_secs: Option<u64>,
    new_ttl_secs: Option<u64>,
    changed_by: &str,
    changed_at: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO tenant_policy_history_config_history
             (tenant, action, old_ttl_secs, new_ttl_secs, changed_by, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            tenant,
            action,
            old_ttl_secs.map(|v| v as i64),
            new_ttl_secs.map(|v| v as i64),
            changed_by,
            changed_at
        ],
    )
    .map_err(|e| format!("failed to record policy history ttl change: {e}"))?;
    Ok(())
}

/// Trims a tenant's policy-history retention-window override change history to its
/// count cap (issue C2).
///
/// Runs in the same transaction as the change that triggered it, so the table
/// never grows unbounded while the newest rows (the ones the audit endpoint
/// serves) are preserved. Only the count cap applies — the override's own window
/// must not expire the audit trail of who set it. Other tenants are never touched.
/// Returns the number of rows removed.
fn prune_policy_history_ttl_history(
    conn: &Connection,
    tenant: &str,
    max: usize,
) -> Result<usize, String> {
    if max == 0 {
        return Ok(0);
    }
    let removed = conn
        .execute(
            "DELETE FROM tenant_policy_history_config_history
         WHERE tenant = ?1
           AND id NOT IN (
               SELECT id FROM tenant_policy_history_config_history
               WHERE tenant = ?1 ORDER BY id DESC LIMIT ?2
           )",
            params![tenant, max as i64],
        )
        .map_err(|e| format!("failed to prune policy history ttl history: {e}"))?;
    Ok(removed)
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

/// Rolls a tenant's budget window when it has elapsed (no-op when disabled).
///
/// A row with `window_started_at = 0` is treated as "window not yet anchored" and
/// is re-anchored to `now`. The roll zeroes both ε and δ.
fn roll_dp_window(
    conn: &Connection,
    tenant: &str,
    window_secs: u64,
    now: i64,
) -> Result<(), String> {
    if window_secs == 0 {
        return Ok(());
    }
    conn.execute(
        "UPDATE dp_budget
         SET spent_epsilon = 0,
             spent_delta = 0,
             window_started_at = ?1,
             updated_at = ?1
         WHERE tenant = ?2
           AND (window_started_at = 0 OR ?1 - window_started_at >= ?3)",
        params![now, tenant, window_secs as i64],
    )
    .map_err(|e| format!("failed to roll dp window: {e}"))?;
    Ok(())
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
            .record_run("hash1", "success", 42, None, "tenant-a")
            .expect("insert");
        let id2 = store
            .record_run("hash2", "failed", 0, Some("boom"), "tenant-a")
            .expect("insert");
        // A different tenant's run must not be visible to tenant-a.
        let id_other = store
            .record_run("hash3", "success", 1, None, "tenant-b")
            .expect("insert");

        let list = store.list_runs(10, "tenant-a").expect("list");
        assert_eq!(list.len(), 2, "{list:?}");
        assert!(list.iter().all(|r| r.tenant == "tenant-a"));
        let first = &list[0];
        assert!(first.id == id2 || first.id == id);
        // Newest first.
        assert_eq!(first.id.max(id2), first.id);

        let got = store.get_run(id, "tenant-a").expect("get").expect("exists");
        assert_eq!(got.status, "success");
        assert_eq!(got.rows, 42);
        assert!(got.error.is_none());

        let got2 = store
            .get_run(id2, "tenant-a")
            .expect("get")
            .expect("exists");
        assert_eq!(got2.status, "failed");
        assert_eq!(got2.error.as_deref(), Some("boom"));

        // Cross-tenant access is denied: tenant-a cannot read tenant-b's run.
        assert!(
            store
                .get_run(id_other, "tenant-a")
                .expect("no err")
                .is_none()
        );
        // tenant-b sees only its own run.
        let b_list = store.list_runs(10, "tenant-b").expect("list");
        assert_eq!(b_list.len(), 1);

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
        assert!(store.get_run(999_999, "").expect("no err").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// DP budget accrues cumulatively and is isolated per tenant (issue C2).
    #[test]
    fn dp_budget_accrues_and_is_tenant_scoped() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_dp_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        // Unknown tenant starts at zero.
        assert_eq!(store.dp_spent("a", 0).expect("read"), (0.0, 0.0));

        store.add_dp_spend("a", 1.5, 0.0, 0).expect("spend 1");
        store.add_dp_spend("a", 0.5, 1e-5, 0).expect("spend 2");
        let (eps, delta) = store.dp_spent("a", 0).expect("read");
        assert!((eps - 2.0).abs() < 1e-12, "eps={eps}");
        assert!((delta - 1e-5).abs() < 1e-15, "delta={delta}");

        // Tenant isolation: b is untouched by a's spend.
        assert_eq!(store.dp_spent("b", 0).expect("read"), (0.0, 0.0));

        // A zero-spend update is a no-op; non-finite values are rejected.
        store.add_dp_spend("a", 0.0, 0.0, 0).expect("no-op");
        assert!((store.dp_spent("a", 0).expect("read").0 - 2.0).abs() < 1e-12);
        assert!(store.add_dp_spend("a", f64::NAN, 0.0, 0).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tenant can reset its accumulated spend; the reset is tenant-scoped (issue C2).
    #[test]
    fn dp_budget_reset_is_tenant_scoped() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_reset_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        store.add_dp_spend("a", 3.0, 1e-5, 0).expect("spend a");
        store.add_dp_spend("b", 1.0, 0.0, 0).expect("spend b");

        store.reset_dp_budget("a").expect("reset a");
        assert_eq!(store.dp_spent("a", 0).expect("read a"), (0.0, 0.0));
        // b is unaffected.
        assert!((store.dp_spent("b", 0).expect("read b").0 - 1.0).abs() < 1e-12);

        // Reset also re-anchors the window.
        assert!(
            store
                .dp_window_started_at("a")
                .expect("anchor")
                .unwrap_or(0)
                > 0
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An elapsed budget window rolls the spend to zero; window 0 keeps cumulative (issue C2).
    #[test]
    fn dp_budget_window_rolls_over_when_elapsed() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_window_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        store.add_dp_spend("a", 2.0, 0.0, 3600).expect("spend");
        assert!((store.dp_spent("a", 3600).expect("read").0 - 2.0).abs() < 1e-12);

        // Age the window anchor by two hours so the one-hour window has elapsed.
        {
            let conn = Connection::open(&db).expect("open raw");
            conn.execute(
                "UPDATE dp_budget SET window_started_at = window_started_at - 7200 WHERE tenant = 'a'",
                [],
            )
            .expect("age window");
        }
        assert_eq!(
            store.dp_spent("a", 3600).expect("read after roll"),
            (0.0, 0.0)
        );

        // Window disabled (0) is cumulative across calls.
        store.add_dp_spend("a", 1.0, 0.0, 0).expect("spend again");
        assert!((store.dp_spent("a", 0).expect("read cumulative").0 - 1.0).abs() < 1e-12);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tenant DP window overrides are stored, isolated per tenant, and clearable (issue C2).
    #[test]
    fn dp_window_override_is_per_tenant_and_clearable() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_dpwin_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        // No override by default.
        assert_eq!(store.get_dp_window("a").expect("read"), None);
        store.set_dp_window("a", 3600).expect("set a");
        // An explicit 0 is a stored override, not "unset".
        store.set_dp_window("b", 0).expect("set b");
        assert_eq!(store.get_dp_window("a").expect("read a"), Some(3600));
        assert_eq!(store.get_dp_window("b").expect("read b"), Some(0));
        assert_eq!(store.get_dp_window("c").expect("read c"), None);

        // Clearing is tenant-scoped and reports whether a row existed.
        assert!(store.clear_dp_window("a").expect("clear a"));
        assert_eq!(store.get_dp_window("a").expect("read a"), None);
        assert!(!store.clear_dp_window("a").expect("clear a again"));
        assert_eq!(store.get_dp_window("b").expect("read b"), Some(0));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Setting a window re-anchors the existing budget row without wiping spend (issue C2).
    #[test]
    fn setting_dp_window_reanchors_without_wiping_spend() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_dpwin_anchor_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        store.add_dp_spend("a", 2.0, 0.0, 0).expect("spend");
        // Age the anchor far beyond the window that is about to be configured.
        {
            let conn = Connection::open(&db).expect("open raw");
            conn.execute(
                "UPDATE dp_budget SET window_started_at = window_started_at - 100000 WHERE tenant = 'a'",
                [],
            )
            .expect("age window");
        }

        store.set_dp_window("a", 3600).expect("set window");
        // The anchor moved to now, so the fresh one-hour window retains the spend.
        assert!((store.dp_spent("a", 3600).expect("read").0 - 2.0).abs() < 1e-12);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tenant policy packs are stored and isolated per tenant (issue C2).
    #[test]
    fn tenant_policies_are_namespaced_per_tenant() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_pol_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        // Unknown tenants have no stored pack.
        assert!(store.get_tenant_policy("a").expect("read").is_none());

        store
            .set_tenant_policy("a", r#"{"id":"a-pack"}"#, "a")
            .expect("write a");
        store
            .set_tenant_policy("b", r#"{"id":"b-pack"}"#, "b")
            .expect("write b");

        // Each tenant reads only its own pack.
        assert_eq!(
            store.get_tenant_policy("a").expect("read a").as_deref(),
            Some(r#"{"id":"a-pack"}"#)
        );
        assert_eq!(
            store.get_tenant_policy("b").expect("read b").as_deref(),
            Some(r#"{"id":"b-pack"}"#)
        );

        // Replacing a pack only affects that tenant.
        store
            .set_tenant_policy("a", r#"{"id":"a-v2"}"#, "a")
            .expect("rewrite a");
        assert_eq!(
            store.get_tenant_policy("a").expect("reread a").as_deref(),
            Some(r#"{"id":"a-v2"}"#)
        );
        assert_eq!(
            store.get_tenant_policy("b").expect("reread b").as_deref(),
            Some(r#"{"id":"b-pack"}"#)
        );

        // Deleting a pack is tenant-scoped.
        assert!(store.delete_tenant_policy("a", "a").expect("delete a"));
        assert!(store.get_tenant_policy("a").expect("read a").is_none());
        assert!(store.get_tenant_policy("b").expect("read b").is_some());
        assert!(!store.delete_tenant_policy("a", "a").expect("delete again"));

        // The change history is append-only and tenant-scoped: set, replace, delete.
        let hist_a = store.list_policy_history("a", 10, 0).expect("history a");
        let actions: Vec<&str> = hist_a.iter().map(|h| h.action.as_str()).collect();
        assert_eq!(actions, vec!["delete", "set", "set"], "{hist_a:?}");
        assert_eq!(hist_a[0].changed_by, "a");
        assert_eq!(
            hist_a[0].old_policy_json.as_deref(),
            Some(r#"{"id":"a-v2"}"#)
        );
        assert!(hist_a[0].new_policy_json.is_none());
        assert!(hist_a[1].old_policy_json.is_some());
        // tenant-b's history is separate and untouched by a's deletes.
        let hist_b = store.list_policy_history("b", 10, 0).expect("history b");
        assert_eq!(hist_b.len(), 1);
        assert_eq!(hist_b[0].action, "set");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The retention cap parser rejects disabled/invalid values (issue C2).
    #[test]
    fn policy_history_max_resolver_falls_back_to_default() {
        assert_eq!(resolve_policy_history_max(None), DEFAULT_POLICY_HISTORY_MAX);
        assert_eq!(resolve_policy_history_max(Some("5")), 5);
        assert_eq!(
            resolve_policy_history_max(Some("0")),
            DEFAULT_POLICY_HISTORY_MAX
        );
        assert_eq!(
            resolve_policy_history_max(Some("not-a-number")),
            DEFAULT_POLICY_HISTORY_MAX
        );
    }

    /// The retention-window parser is opt-in: unset/invalid/zero disable expiry
    /// (issue C2).
    #[test]
    fn policy_history_ttl_resolver_defaults_to_disabled() {
        assert_eq!(resolve_policy_history_ttl(None), 0);
        assert_eq!(resolve_policy_history_ttl(Some("3600")), 3600);
        assert_eq!(resolve_policy_history_ttl(Some("0")), 0);
        assert_eq!(resolve_policy_history_ttl(Some("not-a-number")), 0);
    }

    /// A disabled window never filters any row (issue C2).
    #[test]
    fn history_cutoff_keeps_all_rows_when_disabled() {
        assert_eq!(history_cutoff(0, 1_000_000), 0);
        assert_eq!(history_cutoff(60, 1_000_000), 999_940);
    }

    /// The sweep-interval parser defaults to cleaning and honours an opt-out
    /// (issue C2).
    #[test]
    fn policy_history_sweep_resolver_defaults_on_and_allows_disable() {
        assert_eq!(
            resolve_policy_history_sweep(None),
            DEFAULT_POLICY_HISTORY_SWEEP_SECS
        );
        assert_eq!(resolve_policy_history_sweep(Some("60")), 60);
        assert_eq!(resolve_policy_history_sweep(Some("0")), 0);
        assert_eq!(
            resolve_policy_history_sweep(Some("not-a-number")),
            DEFAULT_POLICY_HISTORY_SWEEP_SECS
        );
    }

    /// The sweep physically removes expired rows for idle tenants without a
    /// write, while an explicit `0` override protects that tenant (issue C2).
    #[test]
    fn policy_history_sweep_removes_expired_rows_for_idle_tenants() {
        let db = unique_db("hist_sweep");
        let store = Store::open_at_with_history(&db, DEFAULT_POLICY_HISTORY_MAX, 3600);

        for i in 1..=2 {
            store
                .set_tenant_policy("a", &format!(r#"{{"id":"a-{i}"}}"#), "a")
                .expect("set a");
        }
        store
            .set_tenant_policy("b", r#"{"id":"b-1"}"#, "b")
            .expect("set b");
        // Tenant c disables time-based expiry with an explicit `0` override.
        store
            .set_policy_history_ttl("c", 0, "c")
            .expect("override c");
        store
            .set_tenant_policy("c", r#"{"id":"c-1"}"#, "c")
            .expect("set c");

        // Age every row past the one-hour window.
        {
            let conn = Connection::open(&db).expect("open raw");
            conn.execute(
                "UPDATE tenant_policy_history SET changed_at = ?1",
                params![now_epoch() - 7200],
            )
            .expect("age rows");
        }

        // No write happens: the sweep alone must clean up the expired rows.
        let removed = store.sweep_expired_policy_history().expect("sweep");
        assert_eq!(removed, 3, "a (2) + b (1) expired rows are removed");

        {
            let conn = Connection::open(&db).expect("open raw");
            let count = |tenant: &str| -> i64 {
                conn.query_row(
                    "SELECT COUNT(*) FROM tenant_policy_history WHERE tenant = ?1",
                    params![tenant],
                    |row| row.get(0),
                )
                .expect("count rows")
            };
            assert_eq!(count("a"), 0);
            assert_eq!(count("b"), 0);
            assert_eq!(
                count("c"),
                1,
                "an explicit 0 override disables expiry for c"
            );
        }

        // A second sweep has nothing left to remove.
        assert_eq!(
            store.sweep_expired_policy_history().expect("sweep again"),
            0
        );

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// The sweep enforces the count cap for idle tenants whose cap was lowered
    /// after their rows were written, covering both the pack history and the
    /// override change history — and discovers override-only tenants (issue C2).
    #[test]
    fn policy_history_sweep_enforces_count_cap_for_idle_tenants() {
        let db = unique_db("hist_sweep_cap");
        // Populate under a generous cap so no write prunes anything.
        let seed = Store::open_at_with_history_max(&db, 100);
        for i in 1..=5 {
            seed.set_tenant_policy("a", &format!(r#"{{"id":"a-{i}"}}"#), "a")
                .expect("set a");
        }
        // Tenant b only ever changed its retention-window override.
        for i in 1..=5 {
            seed.set_policy_history_ttl("b", i, "b").expect("set b ttl");
        }

        // A later deployment lowers the cap; the sweep alone must trim both.
        let store = Store::open_at_with_history_max(&db, 2);
        let removed = store.sweep_expired_policy_history().expect("sweep");
        assert_eq!(
            removed, 6,
            "a: 3 pack rows + b: 3 override rows over the cap"
        );

        assert_eq!(
            store
                .list_policy_history("a", 100, 0)
                .expect("history a")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_policy_history_ttl_history("b", 100, 0)
                .expect("history b")
                .len(),
            2,
            "an override-only tenant is discovered and capped"
        );

        // A second sweep has nothing left to remove.
        assert_eq!(
            store.sweep_expired_policy_history().expect("sweep again"),
            0
        );

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// Each change prunes the tenant's history to the newest `max` rows,
    /// leaving other tenants untouched (issue C2).
    #[test]
    fn policy_history_is_pruned_to_retention_cap() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_hist_cap_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at_with_history_max(&db, 3);

        for i in 1..=5 {
            store
                .set_tenant_policy("a", &format!(r#"{{"id":"a-{i}"}}"#), "a")
                .expect("set a");
        }
        store
            .set_tenant_policy("b", r#"{"id":"b-1"}"#, "b")
            .expect("set b");

        let hist_a = store.list_policy_history("a", 100, 0).expect("history a");
        assert_eq!(hist_a.len(), 3, "cap keeps the newest 3");
        // Newest-first: a-5, a-4, a-3 (a-1/a-2 pruned).
        assert_eq!(
            hist_a[0].new_policy_json.as_deref(),
            Some(r#"{"id":"a-5"}"#)
        );
        assert_eq!(
            hist_a[2].new_policy_json.as_deref(),
            Some(r#"{"id":"a-3"}"#)
        );

        let hist_b = store.list_policy_history("b", 100, 0).expect("history b");
        assert_eq!(hist_b.len(), 1, "pruning is tenant-scoped");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A retention window hides expired rows on read and physically removes them
    /// on the next change, while other tenants are untouched (issue C2).
    #[test]
    fn policy_history_expires_after_retention_window() {
        let db = unique_db("hist_ttl");
        let store = Store::open_at_with_history(&db, DEFAULT_POLICY_HISTORY_MAX, 3600);

        for i in 1..=3 {
            store
                .set_tenant_policy("a", &format!(r#"{{"id":"a-{i}"}}"#), "a")
                .expect("set a");
        }
        store
            .set_tenant_policy("b", r#"{"id":"b-1"}"#, "b")
            .expect("set b");

        // Age a's first two changes past the one-hour window.
        {
            let conn = Connection::open(&db).expect("open raw");
            conn.execute(
                "UPDATE tenant_policy_history SET changed_at = ?1
                 WHERE tenant = 'a' AND id <= 2",
                params![now_epoch() - 7200],
            )
            .expect("age rows");
        }

        // Expired rows are hidden even before a write prunes them.
        let hist_a = store.list_policy_history("a", 100, 0).expect("history a");
        assert_eq!(hist_a.len(), 1, "only the fresh change is served");
        assert_eq!(
            hist_a[0].new_policy_json.as_deref(),
            Some(r#"{"id":"a-3"}"#)
        );

        // The next change prunes the expired rows from disk.
        store
            .set_tenant_policy("a", r#"{"id":"a-4"}"#, "a")
            .expect("set a");
        {
            let conn = Connection::open(&db).expect("open raw");
            let remaining: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tenant_policy_history WHERE tenant = 'a'",
                    [],
                    |row| row.get(0),
                )
                .expect("count a rows");
            assert_eq!(remaining, 2, "expired rows are deleted on write");
        }
        // b's older-than-window risk does not apply; its single row is fresh.
        assert_eq!(
            store
                .list_policy_history("b", 100, 0)
                .expect("history b")
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// A per-tenant retention-window override is stored/read/cleared in isolation
    /// and is distinct from the global default (issue C2).
    #[test]
    fn policy_history_ttl_override_is_per_tenant_and_clearable() {
        let db = unique_db("hist_ttl_override");
        // Global window disabled; only overrides can expire rows.
        let store = Store::open_at(&db);
        assert_eq!(store.policy_history_ttl_default(), 0);

        assert_eq!(store.get_policy_history_ttl("a").expect("read a"), None);
        store.set_policy_history_ttl("a", 3600, "a").expect("set a");
        store.set_policy_history_ttl("b", 0, "b").expect("set b");

        assert_eq!(
            store.get_policy_history_ttl("a").expect("read a"),
            Some(3600)
        );
        // An explicit `0` override differs from having no override.
        assert_eq!(store.get_policy_history_ttl("b").expect("read b"), Some(0));
        assert_eq!(store.get_policy_history_ttl("c").expect("read c"), None);

        assert!(store.clear_policy_history_ttl("a", "a").expect("clear a"));
        assert_eq!(store.get_policy_history_ttl("a").expect("read a"), None);
        assert!(
            !store
                .clear_policy_history_ttl("a", "a")
                .expect("clear a again")
        );
        assert_eq!(store.get_policy_history_ttl("b").expect("read b"), Some(0));

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// Every set/clear of a per-tenant retention-window override appends a
    /// who/when/previous-value record, scoped to the tenant (issue C2).
    #[test]
    fn policy_history_ttl_change_history_records_set_and_clear() {
        let db = unique_db("hist_ttl_change");
        let store = Store::open_at(&db);

        assert!(
            store
                .list_policy_history_ttl_history("a", 100, 0)
                .expect("empty")
                .is_empty()
        );

        store.set_policy_history_ttl("a", 3600, "a").expect("set 1");
        store
            .set_policy_history_ttl("a", 0, "admin")
            .expect("set 2");
        store.set_policy_history_ttl("b", 60, "b").expect("set b");

        let hist = store
            .list_policy_history_ttl_history("a", 100, 0)
            .expect("history a");
        assert_eq!(hist.len(), 2, "newest-first set records");
        assert_eq!(hist[0].action, "set");
        assert_eq!(hist[0].old_ttl_secs, Some(3600));
        assert_eq!(hist[0].new_ttl_secs, Some(0));
        assert_eq!(hist[0].changed_by, "admin");
        assert_eq!(hist[1].action, "set");
        assert_eq!(
            hist[1].old_ttl_secs, None,
            "first set has no previous value"
        );
        assert_eq!(hist[1].new_ttl_secs, Some(3600));
        assert_eq!(hist[1].changed_by, "a");

        assert!(store.clear_policy_history_ttl("a", "a").expect("clear a"));
        let hist = store
            .list_policy_history_ttl_history("a", 100, 0)
            .expect("history a after clear");
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[0].action, "clear");
        assert_eq!(hist[0].old_ttl_secs, Some(0));
        assert_eq!(hist[0].new_ttl_secs, None, "a clear has no new value");

        // A clear that removes nothing records nothing.
        assert!(
            !store
                .clear_policy_history_ttl("a", "a")
                .expect("clear a again")
        );
        assert_eq!(
            store
                .list_policy_history_ttl_history("a", 100, 0)
                .expect("history a after no-op")
                .len(),
            3
        );

        // History is tenant-scoped.
        assert_eq!(
            store
                .list_policy_history_ttl_history("b", 100, 0)
                .expect("history b")
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// The retention-window change history is capped per tenant and pages
    /// newest-first (issue C2).
    #[test]
    fn policy_history_ttl_change_history_is_pruned_and_paginates() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_hist_ttl_cap_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at_with_history_max(&db, 2);

        for i in 1..=4 {
            store.set_policy_history_ttl("a", i, "a").expect("set a");
        }

        let hist = store
            .list_policy_history_ttl_history("a", 100, 0)
            .expect("history a");
        assert_eq!(hist.len(), 2, "cap keeps the newest 2");
        assert_eq!(hist[0].new_ttl_secs, Some(4));
        assert_eq!(hist[1].new_ttl_secs, Some(3));

        let page = store
            .list_policy_history_ttl_history("a", 1, 1)
            .expect("page");
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].new_ttl_secs, Some(3));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A per-tenant retention window expires only that tenant's rows: reads hide
    /// them and the next change prunes them from disk, while another tenant with
    /// no override keeps its aged rows (issue C2).
    #[test]
    fn policy_history_ttl_override_expires_only_that_tenant() {
        let db = unique_db("hist_ttl_tenant");
        // Global window disabled, so only a's override can expire rows.
        let store = Store::open_at(&db);
        store
            .set_policy_history_ttl("a", 3600, "a")
            .expect("override a ttl");

        for i in 1..=3 {
            store
                .set_tenant_policy("a", &format!(r#"{{"id":"a-{i}"}}"#), "a")
                .expect("set a");
        }
        store
            .set_tenant_policy("b", r#"{"id":"b-1"}"#, "b")
            .expect("set b");

        // Age both tenants' rows past the one-hour window.
        {
            let conn = Connection::open(&db).expect("open raw");
            conn.execute(
                "UPDATE tenant_policy_history SET changed_at = ?1
                 WHERE id <= 2",
                params![now_epoch() - 7200],
            )
            .expect("age rows");
        }

        // a's override hides the aged rows; b's disabled global window keeps them.
        let hist_a = store.list_policy_history("a", 100, 0).expect("history a");
        assert_eq!(hist_a.len(), 1, "a's override expires aged rows");
        assert_eq!(
            hist_a[0].new_policy_json.as_deref(),
            Some(r#"{"id":"a-3"}"#)
        );
        assert_eq!(
            store
                .list_policy_history("b", 100, 0)
                .expect("history b")
                .len(),
            1,
            "b has no override, so its aged row survives"
        );

        // The next change prunes a's expired rows using its override.
        store
            .set_tenant_policy("a", r#"{"id":"a-4"}"#, "a")
            .expect("set a");
        {
            let conn = Connection::open(&db).expect("open raw");
            let remaining: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tenant_policy_history WHERE tenant = 'a'",
                    [],
                    |row| row.get(0),
                )
                .expect("count a rows");
            assert_eq!(remaining, 2, "expired rows are deleted on write");
        }

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// History pages newest-first via `limit`/`offset` (issue C2).
    #[test]
    fn policy_history_paginates_newest_first() {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_hist_page_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("xazz.db");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&db);

        for i in 1..=5 {
            store
                .set_tenant_policy("a", &format!(r#"{{"id":"a-{i}"}}"#), "a")
                .expect("set a");
        }

        let page = |offset| {
            store
                .list_policy_history("a", 2, offset)
                .expect("history page")
        };
        let ids = |rows: &[PolicyChangeRecord]| -> Vec<String> {
            rows.iter()
                .map(|r| r.new_policy_json.clone().unwrap_or_default())
                .collect()
        };

        assert_eq!(ids(&page(0)), vec![r#"{"id":"a-5"}"#, r#"{"id":"a-4"}"#]);
        assert_eq!(ids(&page(2)), vec![r#"{"id":"a-3"}"#, r#"{"id":"a-2"}"#]);
        assert_eq!(ids(&page(4)), vec![r#"{"id":"a-1"}"#]);
        assert!(page(6).is_empty(), "offset past the end is empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn unique_db(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xazz_store_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("xazz.db")
    }

    /// A reservation holds the tenant's whole remaining envelope until settled,
    /// and settling bills only the actual spend (issue C2, multi-instance).
    #[test]
    fn dp_reservation_serializes_and_settles_actual_spend() {
        let db = unique_db("reserve");
        let store = Store::open_at(&db);

        // The first reservation takes the full remaining envelope.
        let r1 = store
            .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
            .expect("reserve")
            .expect("first reservation granted");
        assert!((r1.epsilon - 10.0).abs() < 1e-12);
        assert!((r1.delta - 1e-4).abs() < 1e-15);

        // A live reservation blocks a second one for the same tenant...
        assert!(
            store
                .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
                .expect("reserve again")
                .is_none()
        );
        // ...but not for other tenants (isolation).
        assert!(
            store
                .reserve_dp_budget("b", 10.0, 1e-4, 0, 3600)
                .expect("reserve b")
                .is_some()
        );

        // Settling bills only the actual spend and frees the reservation.
        store
            .settle_dp_reservation("a", &r1.id, 2.5, 0.0, 0)
            .expect("settle");
        assert!((store.dp_spent("a", 0).expect("spent").0 - 2.5).abs() < 1e-12);
        // Tenant b's reservation was untouched by a's spend.
        assert!((store.dp_spent("b", 0).expect("spent b").0).abs() < 1e-12);

        // The remaining envelope is available again.
        let r2 = store
            .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
            .expect("reserve after settle")
            .expect("second reservation granted");
        assert!((r2.epsilon - 7.5).abs() < 1e-12);

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// Two `Store`s over the same file model two server instances: only one can
    /// hold a tenant's reservation at a time (issue C2, multi-instance).
    #[test]
    fn dp_reservation_is_exclusive_across_store_instances() {
        let db = unique_db("reserve_x");
        let store1 = Store::open_at(&db);
        let store2 = Store::open_at(&db);

        let r1 = store1
            .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
            .expect("reserve 1")
            .expect("instance 1 granted");
        // The second instance sees the live reservation and must fail-closed.
        assert!(
            store2
                .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
                .expect("reserve 2")
                .is_none()
        );

        store1
            .settle_dp_reservation("a", &r1.id, 1.0, 0.0, 0)
            .expect("settle");

        // Once released, the other instance can reserve what is left.
        let r2 = store2
            .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
            .expect("reserve after settle")
            .expect("instance 2 granted");
        assert!((r2.epsilon - 9.0).abs() < 1e-12);

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// An expired reservation is reclaimed by the next reserve, and a stale
    /// settle cannot release the newer holder's reservation (issue C2).
    #[test]
    fn expired_dp_reservation_is_reclaimed_and_stale_settle_is_inert() {
        let db = unique_db("reserve_ttl");
        let store = Store::open_at(&db);

        let stale = store
            .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
            .expect("reserve")
            .expect("granted");
        // Age the reservation so its TTL has elapsed (simulates a crashed instance).
        {
            let conn = Connection::open(&db).expect("open raw");
            conn.execute(
                "UPDATE dp_reservation SET expires_at = 0 WHERE tenant = 'a'",
                [],
            )
            .expect("expire reservation");
        }

        // The expired reservation is reclaimed and the full envelope is available.
        let fresh = store
            .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
            .expect("reclaim")
            .expect("reclaimed");
        assert!((fresh.epsilon - 10.0).abs() < 1e-12);
        assert_ne!(fresh.id, stale.id);

        // A stale settle still bills its run's spend, but must not release the
        // fresh reservation.
        store
            .settle_dp_reservation("a", &stale.id, 3.0, 0.0, 0)
            .expect("stale settle");
        assert!((store.dp_spent("a", 0).expect("spent").0 - 3.0).abs() < 1e-12);
        assert!(
            store
                .reserve_dp_budget("a", 10.0, 1e-4, 0, 3600)
                .expect("reserve")
                .is_none(),
            "stale settle must not release the fresh holder's reservation"
        );

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }
}
