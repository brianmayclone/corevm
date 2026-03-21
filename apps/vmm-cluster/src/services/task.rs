//! Task management service — tracks long-running operations.

use rusqlite::Connection;
use serde::Serialize;

pub struct TaskService;

#[derive(Debug, Serialize, Clone)]
pub struct TaskInfo {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub progress_pct: i32,
    pub target_type: String,
    pub target_id: String,
    pub initiated_by: Option<i64>,
    pub details_json: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

impl TaskService {
    /// Create a new task.
    pub fn create(db: &Connection, task_type: &str, target_type: &str,
                  target_id: &str, initiated_by: Option<i64>,
                  details_json: Option<&str>) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        db.execute(
            "INSERT INTO tasks (id, task_type, target_type, target_id, initiated_by, details_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![&id, task_type, target_type, target_id, initiated_by, details_json],
        ).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Mark a task as running.
    pub fn start(db: &Connection, id: &str) -> Result<(), String> {
        db.execute(
            "UPDATE tasks SET status = 'running', started_at = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Update task progress.
    pub fn update_progress(db: &Connection, id: &str, progress_pct: i32) -> Result<(), String> {
        db.execute(
            "UPDATE tasks SET progress_pct = ?1 WHERE id = ?2",
            rusqlite::params![progress_pct, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Mark task as completed.
    pub fn complete(db: &Connection, id: &str) -> Result<(), String> {
        db.execute(
            "UPDATE tasks SET status = 'completed', progress_pct = 100, completed_at = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Mark task as failed.
    pub fn fail(db: &Connection, id: &str, error: &str) -> Result<(), String> {
        db.execute(
            "UPDATE tasks SET status = 'failed', error = ?1, completed_at = datetime('now') WHERE id = ?2",
            rusqlite::params![error, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// List recent tasks.
    pub fn list(db: &Connection, limit: u32) -> Result<Vec<TaskInfo>, String> {
        let mut stmt = db.prepare(
            "SELECT id, task_type, status, progress_pct, target_type, target_id, \
                    initiated_by, details_json, error, created_at, started_at, completed_at \
             FROM tasks ORDER BY created_at DESC LIMIT ?1"
        ).map_err(|e| e.to_string())?;

        let tasks = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(TaskInfo {
                id: row.get(0)?, task_type: row.get(1)?, status: row.get(2)?,
                progress_pct: row.get(3)?, target_type: row.get(4)?,
                target_id: row.get(5)?, initiated_by: row.get(6)?,
                details_json: row.get(7)?, error: row.get(8)?,
                created_at: row.get(9)?, started_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(tasks)
    }
}
