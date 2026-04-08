use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{Todo, Topic};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch("
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
                url TEXT,
                embedding BLOB,
                created_at TEXT NOT NULL
            );
        ")?;
        // Migration: add url column for existing databases
        let has_url: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('todos') WHERE name = 'url'",
            [], |r| r.get(0),
        )?;
        if !has_url {
            self.conn.execute("ALTER TABLE todos ADD COLUMN url TEXT", [])?;
        }
        // Migration: add due_date column
        let has_due: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('todos') WHERE name = 'due_date'",
            [], |r| r.get(0),
        )?;
        if !has_due {
            self.conn.execute("ALTER TABLE todos ADD COLUMN due_date TEXT", [])?;
        }
        Ok(())
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
        self.conn.execute("INSERT INTO topics (name) VALUES (?1)", params![name])?;
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
            "INSERT INTO topics (name, embedding) VALUES (?1, ?2)",
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
            "SELECT id, name FROM topics ORDER BY id"
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
        Ok(Todo { id, topic_id, text: text.to_string(), done: false, url: url.map(|s| s.to_string()), due_date: None })
    }

    pub fn set_todo_due_date(&self, id: i64, due_date: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE todos SET due_date = ?1 WHERE id = ?2",
            params![due_date, id],
        )?;
        Ok(())
    }

    pub fn toggle_todo(&self, id: i64) -> Result<bool> {
        self.conn.execute(
            "UPDATE todos SET done = NOT done WHERE id = ?1",
            params![id],
        )?;
        let done: bool = self.conn.query_row(
            "SELECT done FROM todos WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(done)
    }

    pub fn delete_todo(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn todos_for_topic(&self, topic_id: i64) -> Result<Vec<Todo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, url, due_date FROM todos WHERE topic_id = ?1 ORDER BY created_at"
        )?;
        let todos = stmt.query_map(params![topic_id], |row| {
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)? })
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
            Ok(Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: row.get(4)?, due_date: row.get(5)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to list all todos")?;
        Ok(todos)
    }

    pub fn update_embedding(&self, todo_id: i64, embedding: &[f32]) -> Result<()> {
        let blob = encode_embedding(embedding);
        self.conn.execute(
            "UPDATE todos SET embedding = ?1 WHERE id = ?2",
            params![blob, todo_id],
        )?;
        Ok(())
    }

    pub fn all_todos_with_embeddings(&self) -> Result<Vec<(Todo, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, text, done, embedding, created_at FROM todos WHERE embedding IS NOT NULL ORDER BY created_at"
        )?;
        let todos = stmt.query_map([], |row| {
            let blob: Vec<u8> = row.get(4)?;
            let todo = Todo { id: row.get(0)?, topic_id: row.get(1)?, text: row.get(2)?, done: row.get(3)?, url: None, due_date: None };
            let emb = decode_embedding(&blob);
            Ok((todo, emb))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to load todos for search")?;
        Ok(todos)
    }
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
