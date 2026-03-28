//! Database helper types and functions — centralized error handling.
//!
//! All database operations should return `DbResult<T>` instead of using `.ok()`.
//! Use `db_transaction()` for multi-statement atomic operations.

use rusqlite::Connection;

/// Result type for all database operations.
pub type DbResult<T> = Result<T, DbError>;

/// Database error with context.
#[derive(Debug)]
pub struct DbError {
    pub context: String,
    pub source: rusqlite::Error,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.context, self.source)
    }
}

impl std::error::Error for DbError {}

/// Extension trait to add context to rusqlite errors.
pub trait DbContext<T> {
    fn ctx(self, context: &str) -> DbResult<T>;
}

impl<T> DbContext<T> for Result<T, rusqlite::Error> {
    fn ctx(self, context: &str) -> DbResult<T> {
        self.map_err(|e| DbError {
            context: context.to_string(),
            source: e,
        })
    }
}

/// Execute a closure inside a SQLite transaction.
/// Automatically rolls back on error, commits on success.
/// Logs the error context if the transaction fails.
pub fn db_transaction<F, T>(db: &Connection, context: &str, f: F) -> DbResult<T>
where
    F: FnOnce() -> DbResult<T>,
{
    db.execute("BEGIN IMMEDIATE", [])
        .ctx(&format!("{}: BEGIN", context))?;

    match f() {
        Ok(result) => {
            db.execute("COMMIT", [])
                .ctx(&format!("{}: COMMIT", context))?;
            Ok(result)
        }
        Err(e) => {
            if let Err(rb_err) = db.execute("ROLLBACK", []) {
                tracing::error!("{}: ROLLBACK failed: {}", context, rb_err);
            }
            tracing::error!("{}", e);
            Err(e)
        }
    }
}

/// Execute a single DB statement with error logging.
/// Returns the number of rows affected, or logs the error and returns 0.
pub fn db_exec(db: &Connection, sql: &str, params: &[&dyn rusqlite::types::ToSql], context: &str) -> usize {
    match db.execute(sql, params) {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("{}: {}", context, e);
            0
        }
    }
}
