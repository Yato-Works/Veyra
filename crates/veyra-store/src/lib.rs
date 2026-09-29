//! SQLite persistence for Veyra state (Plan.txt sections 4, 38-40).
//!
//! SQLite is the local source for persistent Veyra state. The event log is
//! **append-only**: there is intentionally no API to update or delete
//! historical events. Corrections are appended as new events.

mod projector;

pub use projector::{ChatMessage, ProjectSnapshot, Projector};

use std::path::Path;

use chrono::DateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use veyra_core::{Event, EventType};

/// Errors produced by the persistence layer.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// A Veyra project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: Option<String>,
    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

/// A persisted tool invocation (Plan.txt section 39): audit, replay,
/// verification, failure analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRunRecord {
    pub id: String,
    pub project_id: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub status: String,
    pub started_at: DateTime<chrono::Utc>,
    pub finished_at: Option<DateTime<chrono::Utc>>,
}

/// A persisted AI provider invocation (Plan.txt section 40).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRunRecord {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub model: String,
    pub task_type: Option<String>,
    pub input_metadata: Option<serde_json::Value>,
    pub output_metadata: Option<serde_json::Value>,
    pub status: String,
    pub started_at: DateTime<chrono::Utc>,
    pub finished_at: Option<DateTime<chrono::Utc>>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    root_path TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    objective TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS requirements (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL,
    type TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL,
    FOREIGN KEY(goal_id) REFERENCES goals(id)
);

CREATE TABLE IF NOT EXISTS acceptance_criteria (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL,
    FOREIGN KEY(goal_id) REFERENCES goals(id)
);

CREATE TABLE IF NOT EXISTS project_states (
    project_id TEXT PRIMARY KEY,
    current_phase TEXT,
    current_task TEXT,
    last_verified_at TEXT,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS decisions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    decision TEXT NOT NULL,
    rationale TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS event_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    session_id TEXT,
    event_type TEXT NOT NULL,
    payload JSON NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS tool_runs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    session_id TEXT,
    tool_name TEXT NOT NULL,
    input JSON NOT NULL,
    output JSON,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS provider_runs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    task_type TEXT,
    input_metadata JSON,
    output_metadata JSON,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_event_logs_project ON event_logs(project_id, id);
"#;

/// The SQLite-backed store.
pub struct Store {
    conn: Connection,
}

fn to_utc(s: String) -> rusqlite::Result<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(Into::into)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

fn opt_to_utc(s: Option<String>) -> rusqlite::Result<Option<DateTime<chrono::Utc>>> {
    s.map(to_utc).transpose()
}

const PROJECT_COLS: &str =
    "id, name, root_path, created_at, updated_at";

fn read_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        root_path: row.get(2)?,
        created_at: to_utc(row.get(3)?)?,
        updated_at: to_utc(row.get(4)?)?,
    })
}

impl Store {
    /// Open (and migrate if needed) a store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Open a transient in-memory store (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------

    /// Create a project and return it.
    pub fn create_project(&self, name: &str, root_path: Option<&str>) -> Result<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        self.conn.execute(
            "INSERT INTO projects (id, name, root_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, root_path, now.to_rfc3339(), now.to_rfc3339()],
        )?;
        Ok(Project {
            id,
            name: name.to_string(),
            root_path: root_path.map(str::to_string),
            created_at: now,
            updated_at: now,
        })
    }

    /// Fetch a project by id.
    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE id = ?1");
        self.conn
            .query_row(&sql, params![id], read_project)
            .optional()
            .map_err(Into::into)
    }

    /// List all projects.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let sql = format!(
            "SELECT {PROJECT_COLS} FROM projects ORDER BY created_at"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], read_project)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------------
    // Event log (append-only)
    // ------------------------------------------------------------------

    /// Append an event to the log. Events are immutable once written.
    pub fn append_event(&self, event: &Event) -> Result<()> {
        self.conn.execute(
            "INSERT INTO event_logs (event_id, project_id, session_id, event_type, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id,
                event.project_id,
                event.session_id,
                event.event_type.as_str(),
                serde_json::to_string(&event.payload)?,
                event.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Load the full event history of a project, in log order.
    pub fn events_for_project(&self, project_id: &str) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT event_id, project_id, session_id, event_type, payload, created_at
             FROM event_logs WHERE project_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    to_utc(row.get(5)?)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut events = Vec::new();
        for (id, project_id, session_id, type_str, payload, created_at) in rows {
            match EventType::parse(&type_str) {
                Some(event_type) => {
                    events.push(Event {
                        id,
                        project_id,
                        session_id,
                        event_type,
                        payload: serde_json::from_str::<serde_json::Value>(&payload)?,
                        created_at,
                    });
                }
                None => {
                    eprintln!("[warn] Skipping forward-incompatible event type '{type_str}' for event '{id}' in project '{project_id}'");
                }
            }
        }
        Ok(events)
    }

    /// Load recorded provider runs for a project, in insertion order.
    pub fn provider_runs_for_project(&self, project_id: &str) -> Result<Vec<ProviderRunRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, provider, model, task_type, input_metadata, output_metadata, status, started_at, finished_at
             FROM provider_runs WHERE project_id = ?1 ORDER BY rowid ASC",
        )?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    to_utc(row.get(8)?)?,
                    row.get::<_, Option<String>>(9).and_then(opt_to_utc)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(
                |(id, project_id, provider, model, task_type, input_metadata, output_metadata, status, started_at, finished_at)| {
                    Ok(ProviderRunRecord {
                        id,
                        project_id,
                        provider,
                        model,
                        task_type,
                        input_metadata: input_metadata
                            .map(|s| serde_json::from_str(&s))
                            .transpose()?,
                        output_metadata: output_metadata
                            .map(|s| serde_json::from_str(&s))
                            .transpose()?,
                        status,
                        started_at,
                        finished_at,
                    })
                },
            )
            .collect()
    }

    // ------------------------------------------------------------------
    // Tool / provider execution persistence
    // ------------------------------------------------------------------

    /// Record a tool invocation (audit / replay / verification).
    pub fn record_tool_run(&self, run: &ToolRunRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tool_runs (id, project_id, session_id, tool_name, input, output, status, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                run.id,
                run.project_id,
                run.session_id,
                run.tool_name,
                serde_json::to_string(&run.input)?,
                run.output.as_ref().map(serde_json::to_string).transpose()?,
                run.status,
                run.started_at.to_rfc3339(),
                run.finished_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Record an AI provider invocation (audit / replay / verification).
    pub fn record_provider_run(&self, run: &ProviderRunRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO provider_runs (id, project_id, provider, model, task_type, input_metadata, output_metadata, status, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                run.id,
                run.project_id,
                run.provider,
                run.model,
                run.task_type,
                run.input_metadata.as_ref().map(serde_json::to_string).transpose()?,
                run.output_metadata.as_ref().map(serde_json::to_string).transpose()?,
                run.status,
                run.started_at.to_rfc3339(),
                run.finished_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let p = store
            .create_project("Veyra", Some("c:/Users/smily/Veyra"))
            .unwrap();
        assert_eq!(p.name, "Veyra");

        let fetched = store.get_project(&p.id).unwrap().expect("project exists");
        assert_eq!(fetched, p);
        assert_eq!(store.list_projects().unwrap().len(), 1);
        assert!(store.get_project("missing").unwrap().is_none());
    }

    #[test]
    fn event_log_is_append_only_and_ordered() {
        let store = Store::open_in_memory().unwrap();
        let p = store.create_project("proj", None).unwrap();

        let e1 = Event::now(&p.id, EventType::GoalCreated, json!({"objective": "x"}));
        let e2 = Event::now(&p.id, EventType::UserFeedback, json!({"content": "ここ違う"}));
        store.append_event(&e1).unwrap();
        store.append_event(&e2).unwrap();

        let events = store.events_for_project(&p.id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, e1.id);
        assert_eq!(events[0].event_type, EventType::GoalCreated);
        assert_eq!(events[1].event_type, EventType::UserFeedback);
        assert_eq!(events[1].payload["content"], "ここ違う");
        assert!(events.iter().all(|e| e.project_id == p.id));
    }

    #[test]
    fn events_are_isolated_per_project() {
        let store = Store::open_in_memory().unwrap();
        let p1 = store.create_project("p1", None).unwrap();
        let p2 = store.create_project("p2", None).unwrap();
        store
            .append_event(&Event::now(&p1.id, EventType::UserInput, json!({"a": 1})))
            .unwrap();
        store
            .append_event(&Event::now(&p2.id, EventType::UserInput, json!({"b": 2})))
            .unwrap();
        assert_eq!(store.events_for_project(&p1.id).unwrap().len(), 1);
        assert_eq!(store.events_for_project(&p2.id).unwrap().len(), 1);
    }

    #[test]
    fn tool_and_provider_runs_persist() {
        let store = Store::open_in_memory().unwrap();
        let p = store.create_project("proj", None).unwrap();
        let now = chrono::Utc::now();

        store
            .record_tool_run(&ToolRunRecord {
                id: "tr-1".into(),
                project_id: p.id.clone(),
                session_id: None,
                tool_name: "write_file".into(),
                input: json!({"path": "src/main.rs"}),
                output: Some(json!({"ok": true})),
                status: "completed".into(),
                started_at: now,
                finished_at: Some(now),
            })
            .unwrap();

        store
            .record_provider_run(&ProviderRunRecord {
                id: "pr-1".into(),
                project_id: p.id.clone(),
                provider: "anthropic".into(),
                model: "claude".into(),
                task_type: Some("coding".into()),
                input_metadata: None,
                output_metadata: None,
                status: "completed".into(),
                started_at: now,
                finished_at: Some(now),
            })
            .unwrap();
    }

    #[test]
    fn store_survives_reopen_after_crash() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("veyra.db");

        let p_id = {
            let store = Store::open(&db_path).unwrap();
            let p = store.create_project("survivor", None).unwrap();
            store
                .append_event(&Event::now(
                    &p.id,
                    EventType::GoalCreated,
                    json!({"objective": "y"}),
                ))
                .unwrap();
            p.id
        }; // store dropped here = simulated crash

        let reopened = Store::open(&db_path).unwrap();
        let events = reopened.events_for_project(&p_id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::GoalCreated);
    }

    #[test]
    fn store_tolerates_unknown_event_type_m2() {
        let store = Store::open_in_memory().unwrap();
        let p = store.create_project("forward_compat", None).unwrap();

        // Append a known event
        store
            .append_event(&Event::now(
                &p.id,
                EventType::GoalCreated,
                json!({"objective": "known"}),
            ))
            .unwrap();

        // Directly insert an unknown future event type into the database
        store.conn.execute(
            "INSERT INTO event_logs (event_id, project_id, session_id, event_type, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "evt-future-999",
                p.id,
                None::<String>,
                "quantum_teleportation_completed", // Unknown future event type
                "{}",
                chrono::Utc::now().to_rfc3339(),
            ],
        ).unwrap();

        // Querying events must succeed and return the known event, gracefully skipping unknown future types
        let events = store.events_for_project(&p.id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::GoalCreated);
    }
}





