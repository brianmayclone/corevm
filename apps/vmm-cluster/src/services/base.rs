//! BaseService — common database helpers shared by all services.
//!
//! Provides typed query methods to eliminate boilerplate SQL in services.
//! All services can use these helpers instead of raw rusqlite calls.

use rusqlite::Connection;

/// Common database operations available to all services.
pub struct BaseService;

impl BaseService {
    // ── Scalar Queries ──────────────────────────────────────────────────

    /// Count rows matching a condition.
    pub fn count(db: &Connection, table: &str, condition: &str, params: &[&dyn rusqlite::types::ToSql]) -> i64 {
        let sql = if condition.is_empty() {
            format!("SELECT COUNT(*) FROM {}", table)
        } else {
            format!("SELECT COUNT(*) FROM {} WHERE {}", table, condition)
        };
        db.query_row(&sql, params, |r| r.get(0)).unwrap_or(0)
    }

    /// Sum a column, optionally filtered.
    pub fn sum_i64(db: &Connection, table: &str, column: &str, condition: &str, params: &[&dyn rusqlite::types::ToSql]) -> i64 {
        let sql = if condition.is_empty() {
            format!("SELECT COALESCE(SUM({}), 0) FROM {}", column, table)
        } else {
            format!("SELECT COALESCE(SUM({}), 0) FROM {} WHERE {}", column, table, condition)
        };
        db.query_row(&sql, params, |r| r.get(0)).unwrap_or(0)
    }

    /// Check if a row exists.
    pub fn exists(db: &Connection, table: &str, condition: &str, params: &[&dyn rusqlite::types::ToSql]) -> bool {
        Self::count(db, table, condition, params) > 0
    }

    // ── Timestamp ───────────────────────────────────────────────────────

    /// Get current UTC datetime string (SQLite format).
    pub fn now() -> String {
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }
}
