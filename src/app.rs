use anyhow::Result;
use std::collections::HashMap;

use crate::db::{cosine_similarity, Database};
use crate::due_date;
use crate::embeddings::Embedder;
use crate::models::{Todo, Topic};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Topics,
    Todos,
    Search,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TodoSort {
    Bucketed, // unfinished by created_at, then finished by created_at
    Flat,     // all by created_at
}

impl TodoSort {
    pub fn apply(self, todos: &mut Vec<crate::models::Todo>) {
        // DB always returns todos in created_at order; a stable sort preserves that within buckets.
        if self == TodoSort::Bucketed {
            todos.sort_by_key(|t| t.done);
        }
        // Flat: already in created_at order from DB — nothing to do.
    }

}

pub struct AppInfo {
    pub db_path: String,
    pub model_dir: String,
    pub model_name: String,
}

pub struct App {
    pub topics: Vec<Topic>,
    pub todos: Vec<Todo>,
    pub selected_topic: usize,
    pub selected_todo: usize,
    pub focus: Focus,
    pub mode: Mode,
    pub input: String,
    pub search_query: String,
    pub search_results: Vec<(Todo, f32)>,
    pub selected_search_result: usize,
    pub cursor_pos: usize,
    pub editing: bool,
    pub status_message: Option<String>,
    pub db: Database,
    pub embedder: Option<Embedder>,
    pub should_quit: bool,
    pub confirm_quit: bool,
    pub confirm_delete: Option<Focus>, // Some(focus) means pending delete confirmation
    pub show_info: bool,
    pub info: AppInfo,
    pub todo_sort: TodoSort,
    pub topic_counts: HashMap<i64, (i64, i64)>, // topic_id -> (total, done)
    pub due_popup: bool,
    pub due_input: String,
    pub due_cursor: usize,
}

impl App {
    pub fn new(db: Database, embedder: Option<Embedder>, info: AppInfo) -> Result<Self> {
        let topics = db.list_topics()?;
        let topic_counts = db.topic_counts()?;
        let mut todos = if topics.is_empty() {
            vec![]
        } else {
            db.todos_for_topic(topics[0].id)?
        };
        TodoSort::Bucketed.apply(&mut todos);

        Ok(Self {
            topics,
            todos,
            selected_topic: 0,
            selected_todo: 0,
            focus: Focus::Topics,
            mode: Mode::Normal,
            input: String::new(),
            search_query: String::new(),
            search_results: vec![],
            selected_search_result: 0,
            cursor_pos: 0,
            editing: false,
            status_message: None,
            db,
            embedder,
            should_quit: false,
            confirm_quit: false,
            confirm_delete: None,
            show_info: false,
            info,
            todo_sort: TodoSort::Bucketed,
            topic_counts,
            due_popup: false,
            due_input: String::new(),
            due_cursor: 0,
        })
    }

    pub fn selected_topic_id(&self) -> Option<i64> {
        self.topics.get(self.selected_topic).map(|t| t.id)
    }

    fn embed_with_status(&mut self, text: &str) -> Option<Vec<f32>> {
        match self.embedder.as_ref().map(|e| e.embed(text)) {
            Some(Ok(emb)) => Some(emb),
            Some(Err(e)) => {
                self.status_message = Some(format!("Embedding failed: {}", e));
                None
            }
            None => None,
        }
    }

    pub fn reload_topics(&mut self) -> Result<()> {
        self.topics = self.db.list_topics()?;
        if self.selected_topic >= self.topics.len() {
            self.selected_topic = self.topics.len().saturating_sub(1);
        }
        self.reload_todos()?;
        Ok(())
    }

    pub fn reload_todos(&mut self) -> Result<()> {
        if let Some(id) = self.selected_topic_id() {
            self.todos = self.db.todos_for_topic(id)?;
        } else {
            self.todos = vec![];
        }
        self.todo_sort.clone().apply(&mut self.todos);
        if self.selected_todo >= self.todos.len() {
            self.selected_todo = self.todos.len().saturating_sub(1);
        }
        self.topic_counts = self.db.topic_counts()?;
        Ok(())
    }

    pub fn toggle_todo_sort(&mut self) -> Result<()> {
        let selected_id = self.todos.get(self.selected_todo).map(|t| t.id);
        self.todo_sort = match self.todo_sort {
            TodoSort::Bucketed => TodoSort::Flat,
            TodoSort::Flat => TodoSort::Bucketed,
        };
        // Reload from DB to get canonical created_at order, then apply sort.
        if let Some(id) = self.selected_topic_id() {
            self.todos = self.db.todos_for_topic(id)?;
        }
        self.todo_sort.clone().apply(&mut self.todos);
        // Keep selection on the same item after re-sort.
        if let Some(id) = selected_id {
            if let Some(pos) = self.todos.iter().position(|t| t.id == id) {
                self.selected_todo = pos;
            }
        }
        Ok(())
    }


    pub fn update_topic(&mut self, name: &str) -> Result<()> {
        if let Some(topic) = self.topics.get(self.selected_topic) {
            let id = topic.id;
            self.db.update_topic_name(id, name)?;
            self.reload_topics()?;
        }
        Ok(())
    }

    pub fn update_todo(&mut self, text: &str) -> Result<()> {
        if let Some(todo) = self.todos.get(self.selected_todo) {
            let url = extract_url(text);
            let done = todo.done;
            let id = todo.id;
            let embedding = self.embed_with_status(text);
            self.db.update_todo_text_and_done(id, text, done, url.as_deref(), embedding.as_deref())?;
            self.reload_todos()?;
        }
        Ok(())
    }

    pub fn add_topic(&mut self, name: &str) -> Result<()> {
        let embedding = self.embed_with_status(name);
        let topic = self.db.insert_topic(name, embedding.as_deref())?;
        self.topics.push(topic);
        self.selected_topic = self.topics.len() - 1;
        self.reload_todos()?;
        Ok(())
    }

    pub fn delete_topic(&mut self) -> Result<()> {
        if let Some(id) = self.selected_topic_id() {
            self.db.delete_topic(id)?;
            self.reload_topics()?;
        }
        Ok(())
    }

    pub fn add_todo(&mut self, text: &str) -> Result<()> {
        if let Some(topic_id) = self.selected_topic_id() {
            let url = extract_url(text);
            let embedding = self.embed_with_status(text);
            let todo = self.db.insert_todo(topic_id, text, url.as_deref(), embedding.as_deref())?;
            self.todos.push(todo);
            self.selected_todo = self.todos.len() - 1;
        }
        Ok(())
    }

    pub fn toggle_todo(&mut self) -> Result<()> {
        if let Some(todo) = self.todos.get(self.selected_todo) {
            let new_done = self.db.toggle_todo(todo.id)?;
            if let Some(t) = self.todos.get_mut(self.selected_todo) {
                t.done = new_done;
            }
        }
        Ok(())
    }

    pub fn delete_todo(&mut self) -> Result<()> {
        if let Some(todo) = self.todos.get(self.selected_todo) {
            self.db.delete_todo(todo.id)?;
            self.reload_todos()?;
        }
        Ok(())
    }

    pub fn open_due_popup(&mut self) {
        if self.todos.is_empty() { return; }
        let current = self.todos.get(self.selected_todo)
            .and_then(|t| t.due_date.clone())
            .unwrap_or_default();
        self.due_input = current.clone();
        self.due_cursor = current.chars().count();
        self.due_popup = true;
    }

    pub fn confirm_due_date(&mut self) -> Result<()> {
        let Some(todo) = self.todos.get(self.selected_todo) else { return Ok(()); };
        let id = todo.id;
        match due_date::parse(&self.due_input) {
            Ok(date) => {
                let date_str = date.map(|d| d.format("%Y-%m-%d").to_string());
                self.db.set_todo_due_date(id, date_str.as_deref())?;
                if let Some(t) = self.todos.get_mut(self.selected_todo) {
                    t.due_date = date_str;
                }
                self.due_popup = false;
                self.due_input.clear();
                self.due_cursor = 0;
            }
            Err(msg) => {
                self.status_message = Some(msg);
            }
        }
        Ok(())
    }

    pub fn close_due_popup(&mut self) {
        self.due_popup = false;
        self.due_input.clear();
        self.due_cursor = 0;
    }

    pub fn run_search(&mut self) -> Result<()> {
        if self.search_query.is_empty() {
            self.search_results.clear();
            return Ok(());
        }
        let Some(embedder) = &self.embedder else {
            self.status_message = Some("Embedder not loaded — search unavailable".into());
            return Ok(());
        };
        let query_emb = embedder.embed(&self.search_query)?;
        let all_todos = self.db.all_todos_with_embeddings()?;

        let mut scored: Vec<(Todo, f32)> = all_todos
            .into_iter()
            .map(|(t, emb)| {
                let score = cosine_similarity(&query_emb, &emb);
                (t, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let mut undone: Vec<_> = scored.iter().filter(|(t, _)| !t.done).take(7).cloned().collect();
        let done: Vec<_>       = scored.iter().filter(|(t, _)|  t.done).take(5).cloned().collect();
        undone.extend(done);
        self.search_results = undone;
        self.selected_search_result = 0;
        Ok(())
    }

    pub fn jump_to_search_result(&mut self) -> Result<()> {
        let Some((todo, _)) = self.search_results.get(self.selected_search_result) else {
            return Ok(());
        };
        let todo_id = todo.id;
        let topic_id = todo.topic_id;

        // Find and select the topic
        if let Some(pos) = self.topics.iter().position(|t| t.id == topic_id) {
            self.selected_topic = pos;
            self.reload_todos()?;
            // Find and select the todo
            if let Some(tpos) = self.todos.iter().position(|t| t.id == todo_id) {
                self.selected_todo = tpos;
            }
            self.focus = Focus::Todos;
            self.mode = Mode::Normal;
        }
        Ok(())
    }

    pub fn nav_up(&mut self) {
        match self.focus {
            Focus::Topics => {
                if self.selected_topic > 0 {
                    self.selected_topic -= 1;
                    let _ = self.reload_todos();
                }
            }
            Focus::Todos => {
                if self.selected_todo > 0 {
                    self.selected_todo -= 1;
                }
            }
            Focus::Search => {
                if self.selected_search_result > 0 {
                    self.selected_search_result -= 1;
                }
            }
        }
    }

    pub fn nav_down(&mut self) {
        match self.focus {
            Focus::Topics => {
                if self.selected_topic + 1 < self.topics.len() {
                    self.selected_topic += 1;
                    let _ = self.reload_todos();
                }
            }
            Focus::Todos => {
                if self.selected_todo + 1 < self.todos.len() {
                    self.selected_todo += 1;
                }
            }
            Focus::Search => {
                if self.selected_search_result + 1 < self.search_results.len() {
                    self.selected_search_result += 1;
                }
            }
        }
    }

    /// Label shown in the delete confirmation popup.
    pub fn delete_confirm_label(&self) -> String {
        match self.confirm_delete {
            Some(Focus::Topics) => {
                let name = self.topics.get(self.selected_topic)
                    .map(|t| t.name.as_str()).unwrap_or("this topic");
                format!("Delete topic \"{}\"?", name)
            }
            Some(Focus::Todos) => {
                let text = self.todos.get(self.selected_todo)
                    .map(|t| t.text.as_str()).unwrap_or("this todo");
                let label = if text.len() > 40 { format!("{}…", &text[..40]) } else { text.to_string() };
                format!("Delete \"{}\"?", label)
            }
            _ => "Delete?".into(),
        }
    }

    /// Open the URL of the currently focused item in the default browser.
    pub fn open_url(&mut self) {
        let url = match self.focus {
            Focus::Todos => self.todos.get(self.selected_todo).and_then(|t| t.url.clone()),
            Focus::Search => self.search_results.get(self.selected_search_result)
                .and_then(|(t, _)| t.url.clone()),
            Focus::Topics => None,
        };
        match url {
            Some(u) => {
                if std::process::Command::new("open").arg(&u).spawn().is_err() {
                    // fallback for Linux
                    let _ = std::process::Command::new("xdg-open").arg(&u).spawn();
                }
            }
            None => self.status_message = Some("No URL attached to this item".into()),
        }
    }
}

/// Extract the first https:// URL found in a string.
pub fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '-' && c != '_' && c != '.' && c != '?' && c != '=' && c != '&' && c != '#' && c != ':').to_string())
}
