use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{Migrations, M};

use crate::models::{Todo, Topic};

pub struct Database {
    conn: Connection,
}

const MIGRATIONS: &[M] = &[
    M::up("
        CREATE TABLE IF NOT EXISTS topics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            embedding BLOB
        );
        CREATE TABLE IF NOT EXISTS todos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
            text TEXT NOT NULL,
            done INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
    "),
    M::up("ALTER TABLE todos ADD COLUMN url TEXT;"),
    M::up("ALTER TABLE todos ADD COLUMN due_date TEXT;"),
    M::up("ALTER TABLE todos ADD COLUMN priority INTEGER;"),
    M::up("ALTER TABLE todos ADD COLUMN in_progress INTEGER NOT NULL DEFAULT 0;"),
    M::up("ALTER TABLE todos ADD COLUMN started_at TEXT;"),
    M::up("ALTER TABLE todos ADD COLUMN completed_at TEXT;"),
    M::up("
        CREATE TABLE IF NOT EXISTS comments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
            text TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
    "),
    M::up("ALTER TABLE comments ADD COLUMN url TEXT;"),
    M::up("ALTER TABLE topics ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0; UPDATE topics SET sort_order = id;"),
];

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let mut conn = Connection::open(path)?;

        // Bootstrap: existing databases used manual migrations with no version tracking.
        // If user_version is 0 but the todos table already exists, fast-forward to the
        // last migration that was already applied so rusqlite_migration doesn't re-run them.
        let user_version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if user_version == 0 {
            let version = detect_legacy_version(&conn)?;
            if version > 0 {
                conn.pragma_update(None, "user_version", version)?;
            }
        }

        Migrations::new(MIGRATIONS.to_vec())
            .to_latest(&mut conn)
            .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;
        Ok(Self { conn })
    }

    // --- Topics ---

    pub fn find_or_create_topic(&self, name: &str) -> Result<Topic> {
        let existing: Option<(i64, String)> = self.conn.query_row(
            "SELECT id, name FROM topics WHERE name = ?1",
            params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        if let Some((id, name)) = existing {
            return Ok(Topic { id, name });
        }
        self.conn.execute("INSERT INTO topics (name, sort_order) VALUES (?1, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM topics))", params![name])?;
        Ok(Topic { id: self.conn.last_insert_rowid(), name: name.to_string() })
    }

    /// Find a todo in a topic whose text starts with a given prefix.
    pub fn find_todo_by_prefix(&self, topic_id: i64, prefix: &str) -> Result<Option<(i64, bool)>> {
        let pattern = format!("{}%", prefix);
        let result = self.conn.query_row(
            "SELECT id, done FROM todos WHERE topic_id = ?1 AND text LIKE ?2",
            params![topic_id, pattern],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        Ok(result)
    }

    pub fn update_topic_name(&self, id: i64, name: &str) -> Result<()> {
        self.conn.execute("UPDATE topics SET name = ?1 WHERE id = ?2", params![name, id])?;
        Ok(())
    }

    pub fn update_todo_text_and_done(&self, id: i64, text: &str, done: bool, url: Option<&str>, embedding: Option<&[f32]>) -> Result<()> {
        let blob = embedding.map(encode_embedding);
        self.conn.execute(
            "UPDATE todos SET text = ?1, done = ?2, url = ?3, embedding = COALESCE(?4, embedding) WHERE id = ?5",
            params![text, done, url, blob, id],
        )?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM todos; DELETE FROM topics;")?;
        Ok(())
    }

    pub fn delete_topic_by_name(&self, name: &str) -> Result<()> {
        self.conn.execute("DELETE FROM topics WHERE name = ?1", params![name])?;
        Ok(())
    }

    pub fn insert_topic(&self, name: &str, embedding: Option<&[f32]>) -> Result<Topic> {
        let blob = embedding.map(encode_embedding);
        self.conn.execute(
            "INSERT INTO topics (name, embedding, sort_order) VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM topics))",
            params![name, blob],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Topic { id, name: name.to_string() })
    }

    pub fn delete_topic(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM topics WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_topics(&self) -> Result<Vec<Topic>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name FROM topics ORDER BY sort_order, id"
        )?;
        let topics = stmt.query_map([], |row| {
            Ok(Topic { id: row.get(0)?, name: row.get(1)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to list topics")?;
        Ok(topics)
    }

    // --- Todos ---

    pub fn insert_todo(&self, topic_id: i64, text: &str, url: Option<&str>, embedding: Option<&[f32]>) -> Result<Todo> {
        let blob = embedding.map(encode_embedding);
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO todos (topic_id, text, done, url, embedding, created_at) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
            params![topic_id, text, url, blob, now.to_rfc3339()],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Todo { id, topic_id, text: text.to_string(), done: false, url: url.map(|s| s.to_string()), due_date: None, priority: None, in_progress: false })
    }

    pub fn set_todo_due_date(&self, id: i64, due_date: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE todos SET due_date = ?1 WHERE id = ?2",
            params![due_date, id],
        )?;
        Ok(())
    }

    pub fn set_todo_priority(&self, id: i64, priority: Option<u8>) -> Result<()> {
        self.conn.execute(
            "UPDATE todos SET priority = ?1 WHERE id = ?2",
            params![priority.map(|p| p as i64), id],
        )?;
        Ok(())
    }

    /// Returns all unfinished todos with a due_date in the past, with topic name.
    pub fn overdue_todos(&self) -> Result<Vec<(Todo, String)>> {
        let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.topic_id, t.text, t.done, t.url, t.due_date, t.priority, tp.name, t.in_progress, t.started_at, t.completed_at
             FROM todos t JOIN topics tp ON tp.id = t.topic_id
             WHERE t.done = 0 AND t.due_date IS NOT NULL AND t.due_date < ?1
             ORDER BY t.due_date"
        )?;
        let rows = stmt.query_map(params![today], |row| {
            let todo = Todo {
                id: row.get(0)?,
                topic_id: row.get(1)?,
                text: row.get(2)?,
                done: row.get(3)?,
                url: row.get(4)?,
                due_date: row.get(5)?,
                priority: row.get::<_, Option<i64>>(6)?.map(|p| p as u8),
                in_progress: row.get(8)?,
            };
            let topic_name: String = row.get(7)?;
            Ok((todo, topic_name))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to query overdue todos")?;
        Ok(rows)
    }

    /// Cycles: todo → in_progress → done → todo. Returns (done, in_progress).
    pub fn toggle_todo(&self, id: i64) -> Result<(bool, bool)> {
        let (done, in_progress): (bool, bool) = self.conn.query_row(
            "SELECT done, in_progress FROM todos WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let now = Utc::now().to_rfc3339();
        let (new_done, new_in_progress, started_at, completed_at): (bool, bool, Option<String>, Option<String>) =
            match (done, in_progress) {
                (false, false) => (false, true,  Some(now),  None),       // todo → in_progress
                (false, true)  => (true,  false, None,       Some(now.clone())), // in_progress → done
                _              => (false, false, None,       None),       // done → todo
            };
        self.conn.execute(
            "UPDATE todos SET done = ?1, in_progress = ?2, started_at = COALESCE(?3, started_at), completed_at = ?4 WHERE id = ?5",
            params![new_done, new_in_progress, started_at, completed_at, id],
        )?;
        // Clear started_at when going back to todo
        if !new_done && !new_in_progress {
            self.conn.execute(
                "UPDATE todos SET started_at = NULL, completed_at = NULL WHERE id = ?1",
                params![id],
            )?;
        }
        Ok((new_done, new_in_progress))
    }

    /// Returns (in_progress_count, completed_count) in one query.
    pub fn virtual_topic_counts(&self) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT SUM(in_progress), SUM(done) FROM todos",
            [],
            |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
        )?)
    }

    /// Returns (created_at, started_at, completed_at) for a todo.
    pub fn get_todo_timestamps(&self, id: i64) -> Result<(Option<String>, Option<String>, Option<String>)> {
        let result = self.conn.query_row(
            "SELECT created_at, started_at, completed_at FROM todos WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        Ok(result.unwrap_or((None, None, None)))
    }

    pub fn delete_todo(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn todos_in_progress(&self) -> Result<Vec<Todo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, url, due_date, priority, in_progress FROM todos WHERE in_progress = 1 ORDER BY started_at"
        )?;
        let todos = stmt.query_map([], |row| {
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)?, priority: row.get(6)?, in_progress: row.get(7)? })
        })?.collect::<rusqlite::Result<Vec<_>>>().context("Failed to list in-progress todos")?;
        Ok(todos)
    }

    pub fn todos_completed(&self) -> Result<Vec<Todo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, url, due_date, priority, in_progress FROM todos WHERE done = 1 ORDER BY completed_at DESC"
        )?;
        let todos = stmt.query_map([], |row| {
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)?, priority: row.get(6)?, in_progress: row.get(7)? })
        })?.collect::<rusqlite::Result<Vec<_>>>().context("Failed to list completed todos")?;
        Ok(todos)
    }

    pub fn todos_for_topic(&self, topic_id: i64) -> Result<Vec<Todo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, url, due_date, priority, in_progress FROM todos WHERE topic_id = ?1 ORDER BY created_at"
        )?;
        let todos = stmt.query_map(params![topic_id], |row| {
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)?, priority: row.get(6)?, in_progress: row.get(7)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to list todos")?;
        Ok(todos)
    }

    pub fn topic_counts(&self) -> Result<std::collections::HashMap<i64, (i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT topic_id, COUNT(*), SUM(done) FROM todos GROUP BY topic_id"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })?;
        for row in rows {
            let (topic_id, total, done) = row?;
            map.insert(topic_id, (total, done));
        }
        Ok(map)
    }

    pub fn stats(&self) -> Result<(usize, usize, usize)> {
        let topics: usize = self.conn.query_row("SELECT COUNT(*) FROM topics", [], |r| r.get(0))?;
        let todos: usize = self.conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0))?;
        let indexed: usize = self.conn.query_row("SELECT COUNT(*) FROM todos WHERE embedding IS NOT NULL", [], |r| r.get(0))?;
        Ok((topics, todos, indexed))
    }

    pub fn all_todos(&self) -> Result<Vec<Todo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, url, due_date FROM todos ORDER BY id"
        )?;
        let todos = stmt.query_map([], |row| {
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)?, priority: None, in_progress: false })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to list all todos")?;
        Ok(todos)
    }

    pub fn get_comments_for_todo(&self, todo_id: i64) -> Result<Vec<crate::models::Comment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, text, url, created_at FROM comments WHERE todo_id = ?1 ORDER BY created_at DESC"
        )?;
        let comments = stmt.query_map(params![todo_id], |row| {
            Ok(crate::models::Comment { id: row.get(0)?, text: row.get(1)?, url: row.get(2)?, created_at: row.get(3)? })
        })?.collect::<rusqlite::Result<Vec<_>>>().context("Failed to load comments")?;
        Ok(comments)
    }

    pub fn insert_comment(&self, todo_id: i64, text: &str, url: Option<&str>) -> Result<crate::models::Comment> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO comments (todo_id, text, url, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![todo_id, text, url, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(crate::models::Comment { id, text: text.to_string(), url: url.map(|s| s.to_string()), created_at: now })
    }

    pub fn update_comment(&self, id: i64, text: &str, url: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE comments SET text = ?1, url = ?2 WHERE id = ?3", params![text, url, id])?;
        Ok(())
    }

    pub fn delete_comment(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM comments WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_embedding(&self, todo_id: i64, embedding: &[f32]) -> Result<()> {
        let blob = encode_embedding(embedding);
        self.conn.execute(
            "UPDATE todos SET embedding = ?1 WHERE id = ?2",
            params![blob, todo_id],
        )?;
        Ok(())
    }

    /// Returns all undone todos due this week (overdue through Sunday), ordered by due_date.
    pub fn todos_due_this_week(&self) -> Result<Vec<Todo>> {
        let end = end_of_week().format("%Y-%m-%d").to_string();
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, url, due_date, priority, in_progress FROM todos WHERE done = 0 AND due_date IS NOT NULL AND due_date <= ?1 ORDER BY due_date"
        )?;
        let todos = stmt.query_map(params![end], |row| {
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)?, priority: row.get(6)?, in_progress: row.get(7)? })
        })?.collect::<rusqlite::Result<Vec<_>>>().context("Failed to list due-this-week todos")?;
        Ok(todos)
    }

    /// Returns the count of undone todos due this week (overdue through Sunday).
    pub fn due_this_week_count(&self) -> Result<i64> {
        let end = end_of_week().format("%Y-%m-%d").to_string();
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE done = 0 AND due_date IS NOT NULL AND due_date <= ?1",
            params![end],
            |r| r.get(0),
        )?)
    }

    /// Swap the sort_order of two topics (used for reordering).
    pub fn swap_topic_sort_order(&self, id1: i64, id2: i64) -> Result<()> {
        let s1: i64 = self.conn.query_row(
            "SELECT sort_order FROM topics WHERE id = ?1", params![id1], |r| r.get(0))?;
        let s2: i64 = self.conn.query_row(
            "SELECT sort_order FROM topics WHERE id = ?1", params![id2], |r| r.get(0))?;
        self.conn.execute("UPDATE topics SET sort_order = ?1 WHERE id = ?2", params![s2, id1])?;
        self.conn.execute("UPDATE topics SET sort_order = ?1 WHERE id = ?2", params![s1, id2])?;
        Ok(())
    }

    /// Move a todo to a different topic.
    pub fn move_todo_to_topic(&self, todo_id: i64, new_topic_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE todos SET topic_id = ?1 WHERE id = ?2",
            params![new_topic_id, todo_id],
        )?;
        Ok(())
    }

    pub fn all_todos_with_embeddings(&self) -> Result<Vec<(Todo, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, embedding, url, due_date, priority, in_progress FROM todos WHERE embedding IS NOT NULL ORDER BY id"
        )?;
        let todos = stmt.query_map([], |row| {
            let blob: Vec<u8> = row.get(4)?;
            let todo = Todo {
                id: row.get(0)?,
                topic_id: row.get(1)?,
                text: row.get(2)?,
                done: row.get(3)?,
                url: row.get(5)?,
                due_date: row.get(6)?,
                priority: row.get(7)?,
                in_progress: row.get(8)?,
            };
            let emb = decode_embedding(&blob);
            Ok((todo, emb))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to load todos for search")?;
        Ok(todos)
    }
}

/// Inspects the schema of a legacy database (one with no user_version set) and returns
/// the index of the last migration that has already been applied. The returned value is
/// used to fast-forward rusqlite_migration's version counter so it doesn't re-run
/// migrations that were previously handled manually.
fn detect_legacy_version(conn: &Connection) -> Result<u32> {
    let has_col = |table: &str, col: &str| -> Result<bool> {
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info(?1) WHERE name = ?2",
            params![table, col],
            |r| r.get(0),
        )?)
    };
    let table_exists = |table: &str| -> Result<bool> {
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            params![table],
            |r| r.get(0),
        )?)
    };

    if !table_exists("todos")? { return Ok(0); }
    // Migrations are numbered 1-based (user_version = number of applied migrations).
    if has_col("topics", "sort_order")? { return Ok(10); }
    if has_col("comments", "url")? { return Ok(9); }
    if table_exists("comments")? { return Ok(8); }
    if has_col("todos", "completed_at")? { return Ok(7); }
    if has_col("todos", "started_at")? { return Ok(6); }
    if has_col("todos", "in_progress")? { return Ok(5); }
    if has_col("todos", "priority")? { return Ok(4); }
    if has_col("todos", "due_date")? { return Ok(3); }
    if has_col("todos", "url")? { return Ok(2); }
    Ok(1)
}

fn end_of_week() -> chrono::NaiveDate {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let days_to_sunday = 6 - today.weekday().num_days_from_monday() as i64;
    today + chrono::Duration::days(days_to_sunday)
}

fn encode_embedding(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn decode_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}
