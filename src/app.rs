use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;

use crate::db::{cosine_similarity, Database};
use crate::due_date;
use crate::embeddings::Embedder;
use crate::models::{Todo, Topic};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Topics,
    Todos,
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
    pub fn apply(self, todos: &mut [crate::models::Todo]) {
        match self {
            // Flat: keep DB inserted order — nothing to do (stable).
            TodoSort::Flat => {}

            // Bucketed: within each done/undone group:
            //   sub-group 0 — has priority  → sort by priority, then due_date
            //   sub-group 1 — due_date only  → sort by due_date
            //   sub-group 2 — neither        → stable (DB added-date order)
            TodoSort::Bucketed => {
                let group = |t: &crate::models::Todo| -> u8 {
                    if t.priority.is_some() {
                        0
                    } else if t.due_date.is_some() {
                        1
                    } else {
                        2
                    }
                };
                todos.sort_by(|a, b| {
                    a.done
                        .cmp(&b.done)
                        .then_with(|| group(a).cmp(&group(b)))
                        .then_with(|| match (group(a), group(b)) {
                            (0, 0) => a
                                .priority
                                .cmp(&b.priority)
                                .then_with(|| a.due_date.cmp(&b.due_date)),
                            (1, 1) => a.due_date.cmp(&b.due_date),
                            _ => std::cmp::Ordering::Equal,
                        })
                });
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DetailField {
    Text,
    Priority,
    Due,
    Url,
    NewComment,
    ExistingComment(usize), // index into comments vec (0 = newest)
}

impl DetailField {
    pub fn next(&self, comment_count: usize) -> Self {
        match self {
            Self::Text => Self::Priority,
            Self::Priority => Self::Due,
            Self::Due => Self::Url,
            Self::Url => Self::NewComment,
            Self::NewComment => {
                if comment_count > 0 {
                    Self::ExistingComment(0)
                } else {
                    Self::Text
                }
            }
            Self::ExistingComment(i) => {
                if i + 1 < comment_count {
                    Self::ExistingComment(i + 1)
                } else {
                    Self::Text
                }
            }
        }
    }
    pub fn prev(&self, comment_count: usize) -> Self {
        match self {
            Self::Text => {
                if comment_count > 0 {
                    Self::ExistingComment(comment_count - 1)
                } else {
                    Self::NewComment
                }
            }
            Self::Priority => Self::Text,
            Self::Due => Self::Priority,
            Self::Url => Self::Due,
            Self::NewComment => Self::Url,
            Self::ExistingComment(0) => Self::NewComment,
            Self::ExistingComment(i) => Self::ExistingComment(i - 1),
        }
    }
}

pub struct DetailState {
    pub todo_id: i64,
    pub field: DetailField,
    pub text: String,
    pub text_cursor: usize,
    pub priority: Option<u8>,
    pub due: String,
    pub due_cursor: usize,
    pub url: String,
    pub url_cursor: usize,
    pub created_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    // Comments
    pub comments: Vec<crate::models::Comment>,
    pub new_comment: String,
    pub new_comment_cursor: usize,
    pub comment_edit_text: String,
    pub comment_edit_cursor: usize,
    pub detail_scroll: u16,
}

#[derive(Clone, PartialEq)]
pub enum BriefingSection {
    MustDo,
    InFlight,
    Recommended,
    Waiting,
}

#[derive(Clone)]
pub struct BriefingItem {
    pub todo: Todo,
    pub topic_name: String,
    pub section: BriefingSection,
}

pub struct SyncStatus {
    pub message: String,
    pub done: bool,
    pub error: bool,
    pub spinner_frame: usize,
    pub done_frames: u32, // countdown to auto-clear (~100ms per frame)
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
    pub detail: Option<DetailState>,
    pub move_popup: bool,
    pub move_popup_selected: usize,
    pub sync_popup: bool,
    pub sync_popup_selected: usize,
    pub sync_rx: Option<std::sync::mpsc::Receiver<crate::sync::SyncMsg>>,
    pub sync_status: Option<SyncStatus>,
    pub search_debounce: Option<Instant>,
    pub search_open: bool,
    pub topic_cursor_memory: HashMap<i64, usize>,
    pub last_topic_id: Option<i64>,
    pub show_virtual_topics: bool,
    pub briefing_open: bool,
    pub briefing_items: Vec<BriefingItem>,
    pub selected_briefing: usize,
}

impl App {
    pub fn new(db: Database, embedder: Option<Embedder>, info: AppInfo) -> Result<Self> {
        let mut topics = vec![
            Topic {
                id: -1,
                name: "🔄 In Progress".to_string(),
            },
            Topic {
                id: -2,
                name: "✅ Completed".to_string(),
            },
            Topic {
                id: -3,
                name: "📅 Due This Week".to_string(),
            },
            Topic {
                id: -4,
                name: "📋 All Tasks".to_string(),
            },
        ];
        topics.extend(db.list_topics()?);
        let mut topic_counts = db.topic_counts()?;
        let (in_progress, completed) = db.virtual_topic_counts()?;
        topic_counts.insert(-1, (in_progress, 0));
        topic_counts.insert(-2, (completed, completed));
        let due_this_week = db.due_this_week_count()?;
        topic_counts.insert(-3, (due_this_week, 0));
        let (all_total, all_done) = db.all_todos_count()?;
        topic_counts.insert(-4, (all_total, all_done));
        let mut todos = db.todos_in_progress()?; // first topic is "In Progress"
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
            detail: None,
            move_popup: false,
            move_popup_selected: 0,
            sync_popup: false,
            sync_popup_selected: 0,
            sync_rx: None,
            sync_status: None,
            search_debounce: None,
            search_open: false,
            topic_cursor_memory: HashMap::new(),
            last_topic_id: Some(-1),
            show_virtual_topics: true,
            briefing_open: false,
            briefing_items: vec![],
            selected_briefing: 0,
        })
    }

    pub fn selected_topic_id(&self) -> Option<i64> {
        self.topics.get(self.selected_topic).map(|t| t.id)
    }

    pub fn is_virtual_topic(&self) -> bool {
        matches!(
            self.selected_topic_id(),
            Some(-1) | Some(-2) | Some(-3) | Some(-4)
        )
    }

    pub fn toggle_virtual_topics(&mut self) -> Result<()> {
        let current_id = self.selected_topic_id();
        self.show_virtual_topics = !self.show_virtual_topics;
        self.reload_topics_list()?;
        // Restore selection; fall back to first real topic if current was a now-hidden virtual
        let new_pos = current_id
            .and_then(|id| self.topics.iter().position(|t| t.id == id))
            .or_else(|| self.topics.iter().position(|t| t.id > 0))
            .unwrap_or(0)
            .min(self.topics.len().saturating_sub(1));
        self.selected_topic = new_pos;
        self.reload_todos()?;
        Ok(())
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

    fn reembed_todo(&mut self, todo_id: i64, todo_text: &str) -> Result<()> {
        let comments = self.db.all_comment_texts_by_todo(todo_id)?;
        let embed_text = build_embed_text(todo_text, &comments);
        if let Some(emb) = self.embed_with_status(&embed_text) {
            self.db.update_embedding(todo_id, &emb)?;
        }
        Ok(())
    }

    fn reload_topics_list(&mut self) -> Result<()> {
        let mut topics: Vec<Topic> = if self.show_virtual_topics {
            vec![
                Topic {
                    id: -1,
                    name: "🔄 In Progress".to_string(),
                },
                Topic {
                    id: -2,
                    name: "✅ Completed".to_string(),
                },
                Topic {
                    id: -3,
                    name: "📅 Due This Week".to_string(),
                },
                Topic {
                    id: -4,
                    name: "📋 All Tasks".to_string(),
                },
            ]
        } else {
            vec![]
        };
        topics.extend(self.db.list_topics()?);
        self.topics = topics;
        if self.selected_topic >= self.topics.len() {
            self.selected_topic = self.topics.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn reload_topics(&mut self) -> Result<()> {
        self.reload_topics_list()?;
        self.reload_todos()?;
        Ok(())
    }

    pub fn reload_todos(&mut self) -> Result<()> {
        let new_id = self.selected_topic_id();

        // Save cursor position when switching away from a topic
        if self.last_topic_id != new_id {
            if let Some(prev) = self.last_topic_id {
                self.topic_cursor_memory.insert(prev, self.selected_todo);
            }
        }

        self.todos = match new_id {
            Some(-1) => self.db.todos_in_progress()?,
            Some(-2) => self.db.todos_completed()?,
            Some(-3) => self.db.todos_due_this_week()?,
            Some(-4) => self.db.todos_all()?,
            Some(id) => self.db.todos_for_topic(id)?,
            None => vec![],
        };
        self.todo_sort.clone().apply(&mut self.todos);

        let max = self.todos.len().saturating_sub(1);
        if self.last_topic_id != new_id {
            // Topic switch: restore saved cursor or go to top
            self.selected_todo = new_id
                .and_then(|id| self.topic_cursor_memory.get(&id).copied())
                .unwrap_or(0)
                .min(max);
        } else {
            // Same-topic reload (add/delete/edit): just clamp
            self.selected_todo = self.selected_todo.min(max);
        }

        self.topic_counts = self.db.topic_counts()?;
        let (in_progress, completed) = self.db.virtual_topic_counts()?;
        self.topic_counts.insert(-1, (in_progress, 0));
        self.topic_counts.insert(-2, (completed, completed));
        let due_this_week = self.db.due_this_week_count()?;
        self.topic_counts.insert(-3, (due_this_week, 0));
        let (all_total, all_done) = self.db.all_todos_count()?;
        self.topic_counts.insert(-4, (all_total, all_done));

        self.last_topic_id = new_id;
        Ok(())
    }

    pub fn toggle_todo_sort(&mut self) -> Result<()> {
        let selected_id = self.todos.get(self.selected_todo).map(|t| t.id);
        self.todo_sort = match self.todo_sort {
            TodoSort::Bucketed => TodoSort::Flat,
            TodoSort::Flat => TodoSort::Bucketed,
        };
        self.todos = match self.selected_topic_id() {
            Some(-1) => self.db.todos_in_progress()?,
            Some(-2) => self.db.todos_completed()?,
            Some(-3) => self.db.todos_due_this_week()?,
            Some(-4) => self.db.todos_all()?,
            Some(id) => self.db.todos_for_topic(id)?,
            None => vec![],
        };
        self.todo_sort.clone().apply(&mut self.todos);
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
            let (clean, priority) = extract_priority(text);
            let url = extract_url(&clean).or_else(|| todo.url.clone());
            let done = todo.done;
            let id = todo.id;
            let comments = self.db.all_comment_texts_by_todo(id)?;
            let embed_text = build_embed_text(&clean, &comments);
            let embedding = self.embed_with_status(&embed_text);
            self.db.update_todo_text_and_done(
                id,
                &clean,
                done,
                url.as_deref(),
                embedding.as_deref(),
            )?;
            if priority.is_some() {
                self.db.set_todo_priority(id, priority)?;
            }
            self.reload_todos()?;
        }
        Ok(())
    }

    pub fn add_topic(&mut self, name: &str) -> Result<()> {
        let topic = self.db.insert_topic(name, None)?;
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
            let (clean, priority) = extract_priority(text);
            let url = extract_url(&clean);
            let embedding = self.embed_with_status(&clean);
            let mut todo =
                self.db
                    .insert_todo(topic_id, &clean, url.as_deref(), embedding.as_deref())?;
            if priority.is_some() {
                self.db.set_todo_priority(todo.id, priority)?;
                todo.priority = priority;
            }
            self.todos.push(todo);
            self.selected_todo = self.todos.len() - 1;
        }
        Ok(())
    }

    pub fn cycle_priority(&mut self) -> Result<()> {
        if let Some(todo) = self.todos.get(self.selected_todo) {
            let next = match todo.priority {
                None => Some(1),
                Some(1) => Some(2),
                Some(2) => Some(3),
                _ => None,
            };
            let id = todo.id;
            self.db.set_todo_priority(id, next)?;
            if let Some(t) = self.todos.get_mut(self.selected_todo) {
                t.priority = next;
            }
        }
        Ok(())
    }

    pub fn toggle_todo(&mut self) -> Result<()> {
        if let Some(todo) = self.todos.get(self.selected_todo) {
            let (new_done, new_in_progress, new_blocked) = self.db.toggle_todo(todo.id)?;
            if let Some(t) = self.todos.get_mut(self.selected_todo) {
                t.done = new_done;
                t.in_progress = new_in_progress;
                t.blocked = new_blocked;
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

    pub fn open_detail(&mut self) {
        let Some(todo) = self.todos.get(self.selected_todo) else {
            return;
        };
        let text = todo.text.clone();
        let todo_id = todo.id;
        let priority = todo.priority;
        let due = todo.due_date.clone().unwrap_or_default();
        let due_cursor = todo
            .due_date
            .as_deref()
            .map(|s| s.chars().count())
            .unwrap_or(0);
        let url = todo.url.clone().unwrap_or_default();
        let url_cursor = todo.url.as_deref().map(|s| s.chars().count()).unwrap_or(0);
        let (created_at, started_at, completed_at) = self
            .db
            .get_todo_timestamps(todo_id)
            .unwrap_or((None, None, None));
        let comments = self.db.get_comments_for_todo(todo_id).unwrap_or_default();
        self.detail = Some(DetailState {
            todo_id,
            field: DetailField::Text,
            text_cursor: 0,
            text,
            priority,
            due,
            due_cursor,
            url,
            url_cursor,
            created_at,
            started_at,
            completed_at,
            comments,
            new_comment: String::new(),
            new_comment_cursor: 0,
            comment_edit_text: String::new(),
            comment_edit_cursor: 0,
            detail_scroll: 0,
        });
    }

    pub fn confirm_detail(&mut self) -> Result<()> {
        let (id, due_str, text, priority, url, comment_texts) = {
            let Some(d) = &self.detail else {
                return Ok(());
            };
            let url = if d.url.is_empty() {
                None
            } else {
                Some(d.url.clone())
            };
            let comment_texts: Vec<String> = d.comments.iter().map(|c| c.text.clone()).collect();
            (
                d.todo_id,
                d.due.clone(),
                d.text.clone(),
                d.priority,
                url,
                comment_texts,
            )
        };

        let due_date = if due_str.is_empty() {
            Ok(None)
        } else {
            due_date::parse(&due_str).map(|opt| opt.map(|date| date.format("%Y-%m-%d").to_string()))
        };
        let due_date = match due_date {
            Ok(v) => v,
            Err(msg) => {
                self.status_message = Some(msg);
                return Ok(());
            }
        };

        let embed_text = build_embed_text(&text, &comment_texts);
        let embedding = self.embed_with_status(&embed_text);
        if let Some(todo) = self.todos.iter().find(|t| t.id == id) {
            self.db.update_todo_text_and_done(
                id,
                &text,
                todo.done,
                url.as_deref(),
                embedding.as_deref(),
            )?;
        }
        self.db.set_todo_priority(id, priority)?;
        self.db.set_todo_due_date(id, due_date.as_deref())?;

        self.detail = None;
        self.reload_todos()?;
        Ok(())
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
    }

    /// Bump the selected todo's due date by `days` (positive = forward, negative = back).
    /// If no due date is set, `+` uses today as the base; `-` does nothing.
    pub fn snooze_due_date(&mut self, days: i64) -> Result<()> {
        let Some(todo) = self.todos.get(self.selected_todo) else {
            return Ok(());
        };
        let base = match todo
            .due_date
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        {
            Some(d) => d,
            None => {
                if days < 0 {
                    return Ok(());
                }
                chrono::Local::now().date_naive()
            }
        };
        let new_date = base + chrono::Duration::days(days);
        let date_str = new_date.format("%Y-%m-%d").to_string();
        let id = todo.id;
        self.db.set_todo_due_date(id, Some(&date_str))?;
        if let Some(t) = self.todos.get_mut(self.selected_todo) {
            t.due_date = Some(date_str);
        }
        Ok(())
    }

    pub fn move_topic_up(&mut self) -> Result<()> {
        let Some(topic) = self.topics.get(self.selected_topic) else {
            return Ok(());
        };
        if topic.id <= 0 {
            return Ok(());
        }
        let curr_id = topic.id;
        let prev_idx = self.topics[..self.selected_topic]
            .iter()
            .rposition(|t| t.id > 0);
        if let Some(prev_idx) = prev_idx {
            let prev_id = self.topics[prev_idx].id;
            self.db.swap_topic_sort_order(curr_id, prev_id)?;
            self.reload_topics_list()?;
            if let Some(new_pos) = self.topics.iter().position(|t| t.id == curr_id) {
                self.selected_topic = new_pos;
            }
            self.reload_todos()?;
        }
        Ok(())
    }

    pub fn move_topic_down(&mut self) -> Result<()> {
        let Some(topic) = self.topics.get(self.selected_topic) else {
            return Ok(());
        };
        if topic.id <= 0 {
            return Ok(());
        }
        let curr_id = topic.id;
        let next_idx = self.topics[self.selected_topic + 1..]
            .iter()
            .position(|t| t.id > 0)
            .map(|i| self.selected_topic + 1 + i);
        if let Some(next_idx) = next_idx {
            let next_id = self.topics[next_idx].id;
            self.db.swap_topic_sort_order(curr_id, next_id)?;
            self.reload_topics_list()?;
            if let Some(new_pos) = self.topics.iter().position(|t| t.id == curr_id) {
                self.selected_topic = new_pos;
            }
            self.reload_todos()?;
        }
        Ok(())
    }

    pub fn open_due_popup(&mut self) {
        if self.todos.is_empty() {
            return;
        }
        let current = self
            .todos
            .get(self.selected_todo)
            .and_then(|t| t.due_date.clone())
            .unwrap_or_default();
        self.due_input = current.clone();
        self.due_cursor = current.chars().count();
        self.due_popup = true;
    }

    pub fn confirm_due_date(&mut self) -> Result<()> {
        let Some(todo) = self.todos.get(self.selected_todo) else {
            return Ok(());
        };
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

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut undone: Vec<_> = scored
            .iter()
            .filter(|(t, _)| !t.done)
            .take(7)
            .cloned()
            .collect();
        let done: Vec<_> = scored
            .iter()
            .filter(|(t, _)| t.done)
            .take(5)
            .cloned()
            .collect();
        undone.extend(done);

        // Secondary pass: surface todos whose comments contain the raw query text.
        let comment_hits = self
            .db
            .todos_with_comment_matching(&self.search_query.clone())?;
        let seen_ids: std::collections::HashSet<i64> = undone.iter().map(|(t, _)| t.id).collect();
        for todo in comment_hits {
            if !seen_ids.contains(&todo.id) {
                undone.push((todo, 0.0));
            }
        }

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

    pub fn nav_top(&mut self) {
        match self.focus {
            Focus::Topics => {
                self.selected_topic = 0;
                let _ = self.reload_todos();
            }
            Focus::Todos => {
                self.selected_todo = 0;
            }
        }
    }

    pub fn nav_bottom(&mut self) {
        match self.focus {
            Focus::Topics => {
                self.selected_topic = self.topics.len().saturating_sub(1);
                let _ = self.reload_todos();
            }
            Focus::Todos => {
                self.selected_todo = self.todos.len().saturating_sub(1);
            }
        }
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
        }
    }

    /// Label shown in the delete confirmation popup.
    pub fn delete_confirm_label(&self) -> String {
        match self.confirm_delete {
            Some(Focus::Topics) => {
                let name = self
                    .topics
                    .get(self.selected_topic)
                    .map(|t| t.name.as_str())
                    .unwrap_or("this topic");
                format!("Delete topic \"{}\"?", name)
            }
            Some(Focus::Todos) => {
                let text = self
                    .todos
                    .get(self.selected_todo)
                    .map(|t| t.text.as_str())
                    .unwrap_or("this todo");
                let truncated: String = text.chars().take(40).collect();
                let label = if truncated.len() < text.len() {
                    format!("{}…", truncated)
                } else {
                    text.to_string()
                };
                format!("Delete \"{}\"?", label)
            }
            _ => "Delete?".into(),
        }
    }

    pub fn save_new_comment(&mut self) -> Result<()> {
        let Some(d) = &mut self.detail else {
            return Ok(());
        };
        let text = d.new_comment.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        let todo_id = d.todo_id;
        let todo_text = d.text.clone();
        let url = extract_url(&text);
        let comment = self.db.insert_comment(todo_id, &text, url.as_deref())?;
        {
            let d = self.detail.as_mut().unwrap();
            d.comments.insert(0, comment); // prepend (newest first)
            d.new_comment.clear();
            d.new_comment_cursor = 0;
        }
        self.reembed_todo(todo_id, &todo_text)?;
        Ok(())
    }

    pub fn delete_selected_comment(&mut self) -> Result<()> {
        let Some(d) = &self.detail else {
            return Ok(());
        };
        let DetailField::ExistingComment(i) = d.field.clone() else {
            return Ok(());
        };
        let comment_id = d.comments[i].id;
        let todo_id = d.todo_id;
        let todo_text = d.text.clone();
        self.db.delete_comment(comment_id)?;
        {
            let d = self.detail.as_mut().unwrap();
            d.comments.remove(i);
            // Move focus
            let new_count = d.comments.len();
            d.field = if new_count == 0 {
                DetailField::NewComment
            } else if i < new_count {
                DetailField::ExistingComment(i)
            } else {
                DetailField::ExistingComment(new_count - 1)
            };
        }
        self.reembed_todo(todo_id, &todo_text)?;
        Ok(())
    }

    pub fn save_comment_edit(&mut self) -> Result<()> {
        let Some(d) = &self.detail else {
            return Ok(());
        };
        let DetailField::ExistingComment(i) = d.field.clone() else {
            return Ok(());
        };
        let text = d.comment_edit_text.trim().to_string();
        let comment_id = d.comments[i].id;
        let todo_id = d.todo_id;
        let todo_text = d.text.clone();
        if text.is_empty() {
            // treat as delete
            self.db.delete_comment(comment_id)?;
            {
                let d = self.detail.as_mut().unwrap();
                d.comments.remove(i);
                let new_count = d.comments.len();
                d.field = if new_count == 0 {
                    DetailField::NewComment
                } else if i < new_count {
                    DetailField::ExistingComment(i)
                } else {
                    DetailField::ExistingComment(new_count - 1)
                };
            }
        } else {
            let url = extract_url(&text);
            self.db.update_comment(comment_id, &text, url.as_deref())?;
            {
                let d = self.detail.as_mut().unwrap();
                d.comments[i].text = text;
                d.comments[i].url = url;
            }
        }
        self.reembed_todo(todo_id, &todo_text)?;
        Ok(())
    }

    /// Called when tabbing into ExistingComment(i) — loads edit buffer.
    pub fn enter_comment_edit(&mut self, i: usize) {
        let Some(d) = &mut self.detail else { return };
        if i < d.comments.len() {
            d.comment_edit_text = d.comments[i].text.clone();
            d.comment_edit_cursor = d.comment_edit_text.chars().count();
        }
    }

    /// Returns real topics eligible as move targets for the selected todo (excludes the todo's current topic).
    pub fn move_popup_topics(&self) -> Vec<Topic> {
        let current_topic_id = self.todos.get(self.selected_todo).map(|t| t.topic_id);
        self.topics
            .iter()
            .filter(|t| t.id > 0 && Some(t.id) != current_topic_id)
            .cloned()
            .collect()
    }

    pub fn open_move_popup(&mut self) {
        if self.todos.is_empty() {
            return;
        }
        if self.move_popup_topics().is_empty() {
            self.status_message = Some("No other topics to move to".into());
            return;
        }
        self.move_popup_selected = 0;
        self.move_popup = true;
    }

    pub fn close_move_popup(&mut self) {
        self.move_popup = false;
    }

    pub fn confirm_move_todo(&mut self) -> Result<()> {
        let target_id = {
            let targets = self.move_popup_topics();
            targets.get(self.move_popup_selected).map(|t| t.id)
        };
        let Some(target_id) = target_id else {
            return Ok(());
        };
        let Some(todo) = self.todos.get(self.selected_todo) else {
            return Ok(());
        };
        let todo_id = todo.id;
        self.db.move_todo_to_topic(todo_id, target_id)?;
        self.move_popup = false;
        self.reload_todos()?;
        Ok(())
    }

    /// Open the URL of the currently focused item in the default browser.
    pub fn open_url(&mut self) {
        let url = if let Some(d) = &self.detail {
            if let DetailField::ExistingComment(i) = &d.field {
                d.comments.get(*i).and_then(|c| c.url.clone())
            } else {
                if d.url.is_empty() {
                    None
                } else {
                    Some(d.url.clone())
                }
            }
        } else if self.search_open {
            self.search_results
                .get(self.selected_search_result)
                .and_then(|(t, _)| t.url.clone())
        } else {
            match self.focus {
                Focus::Todos => self
                    .todos
                    .get(self.selected_todo)
                    .and_then(|t| t.url.clone()),
                Focus::Topics => None,
            }
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

    pub fn open_sync_popup(&mut self) {
        if self.sync_rx.is_some() {
            self.status_message = Some("Sync already in progress".into());
            return;
        }
        self.sync_popup = true;
        self.sync_popup_selected = 0;
    }

    pub fn close_sync_popup(&mut self) {
        self.sync_popup = false;
    }

    pub fn start_sync(&mut self, kind: crate::sync::SyncKind) {
        let rx = crate::sync::start(
            kind,
            self.info.db_path.clone(),
            std::path::PathBuf::from(&self.info.model_dir),
        );
        self.sync_rx = Some(rx);
        self.sync_status = Some(SyncStatus {
            message: format!("Starting {}…", kind.label()),
            done: false,
            error: false,
            spinner_frame: 0,
            done_frames: 0,
        });
        self.sync_popup = false;
    }

    pub fn poll_sync(&mut self) -> Result<()> {
        use crate::sync::SyncMsg;

        let msgs: Vec<SyncMsg> = {
            if let Some(rx) = &self.sync_rx {
                let mut v = Vec::new();
                while let Ok(msg) = rx.try_recv() {
                    v.push(msg);
                }
                v
            } else {
                Vec::new()
            }
        };

        for msg in msgs {
            match msg {
                SyncMsg::Status(s) => {
                    if let Some(ss) = &mut self.sync_status {
                        ss.message = s;
                    }
                }
                SyncMsg::Done => {
                    self.sync_rx = None;
                    if let Some(ss) = &mut self.sync_status {
                        ss.done = true;
                        ss.message = "Sync complete ✓".into();
                        ss.done_frames = 30; // ~3 s
                    }
                    self.reload_topics()?;
                }
                SyncMsg::Error(e) => {
                    self.sync_rx = None;
                    if let Some(ss) = &mut self.sync_status {
                        ss.done = true;
                        ss.error = true;
                        ss.message = format!("Sync error: {}", e);
                        ss.done_frames = 50; // ~5 s for errors
                    }
                }
            }
        }

        // Tick spinner / countdown
        if let Some(ss) = &mut self.sync_status {
            if !ss.done {
                ss.spinner_frame = ss.spinner_frame.wrapping_add(1);
            } else if ss.done_frames > 0 {
                ss.done_frames -= 1;
                if ss.done_frames == 0 {
                    self.sync_status = None;
                }
            }
        }
        Ok(())
    }

    // --- Daily briefing ---

    pub fn open_briefing(&mut self) -> Result<()> {
        let today = chrono::Local::now().date_naive();
        let todos_data = self.db.all_undone_todos_with_topics()?;

        let (blocked_data, actionable): (Vec<_>, Vec<_>) =
            todos_data.into_iter().partition(|(t, _, _)| t.blocked);

        let mut must_do_data: Vec<(Todo, String, Option<Vec<f32>>)> = Vec::new();
        let mut in_flight_data: Vec<(Todo, String, Option<Vec<f32>>)> = Vec::new();
        let mut other_data: Vec<(Todo, String, Option<Vec<f32>>)> = Vec::new();

        for (todo, topic_name, emb) in actionable {
            let is_must = todo
                .due_date
                .as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| d <= today)
                .unwrap_or(false);
            if is_must {
                must_do_data.push((todo, topic_name, emb));
            } else if todo.in_progress {
                in_flight_data.push((todo, topic_name, emb));
            } else {
                other_data.push((todo, topic_name, emb));
            }
        }

        must_do_data.sort_by(|a, b| {
            briefing_score(&b.0)
                .partial_cmp(&briefing_score(&a.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        in_flight_data.sort_by(|a, b| {
            briefing_score(&b.0)
                .partial_cmp(&briefing_score(&a.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        other_data.sort_by(|a, b| {
            briefing_score(&b.0)
                .partial_cmp(&briefing_score(&a.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        must_do_data.truncate(4);
        other_data.truncate(20);

        let seeds: Vec<_> = must_do_data
            .iter()
            .chain(in_flight_data.iter())
            .map(|(t, n, e)| (t.clone(), n.clone(), e.clone()))
            .collect();
        let recommended_data = mmr_select(&seeds, other_data, 5);

        let mut items: Vec<BriefingItem> = Vec::new();
        for (todo, topic_name, _) in must_do_data {
            items.push(BriefingItem {
                todo,
                topic_name,
                section: BriefingSection::MustDo,
            });
        }
        for (todo, topic_name, _) in in_flight_data {
            items.push(BriefingItem {
                todo,
                topic_name,
                section: BriefingSection::InFlight,
            });
        }
        for (todo, topic_name, _) in recommended_data {
            items.push(BriefingItem {
                todo,
                topic_name,
                section: BriefingSection::Recommended,
            });
        }
        for (todo, topic_name, _) in blocked_data {
            items.push(BriefingItem {
                todo,
                topic_name,
                section: BriefingSection::Waiting,
            });
        }

        self.briefing_items = items;
        self.selected_briefing = 0;
        self.briefing_open = true;
        Ok(())
    }

    pub fn close_briefing(&mut self) {
        self.briefing_open = false;
    }

    pub fn briefing_toggle_todo(&mut self) -> Result<()> {
        let id = self
            .briefing_items
            .get(self.selected_briefing)
            .map(|i| i.todo.id);
        if let Some(id) = id {
            let (new_done, new_in_progress, new_blocked) = self.db.toggle_todo(id)?;
            if let Some(item) = self.briefing_items.get_mut(self.selected_briefing) {
                item.todo.done = new_done;
                item.todo.in_progress = new_in_progress;
                item.todo.blocked = new_blocked;
            }
            if let Some(t) = self.todos.iter_mut().find(|t| t.id == id) {
                t.done = new_done;
                t.in_progress = new_in_progress;
                t.blocked = new_blocked;
            }
        }
        Ok(())
    }

    pub fn briefing_snooze(&mut self, days: i64) -> Result<()> {
        let Some(item) = self.briefing_items.get(self.selected_briefing) else {
            return Ok(());
        };
        let id = item.todo.id;
        let base = match item
            .todo
            .due_date
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        {
            Some(d) => d,
            None => {
                if days < 0 {
                    return Ok(());
                }
                chrono::Local::now().date_naive()
            }
        };
        let new_date = base + chrono::Duration::days(days);
        let date_str = new_date.format("%Y-%m-%d").to_string();
        self.db.set_todo_due_date(id, Some(&date_str))?;
        if let Some(item) = self.briefing_items.get_mut(self.selected_briefing) {
            item.todo.due_date = Some(date_str.clone());
        }
        if let Some(t) = self.todos.iter_mut().find(|t| t.id == id) {
            t.due_date = Some(date_str);
        }
        Ok(())
    }

    pub fn briefing_jump(&mut self) -> Result<()> {
        let Some(item) = self.briefing_items.get(self.selected_briefing) else {
            self.briefing_open = false;
            return Ok(());
        };
        let topic_id = item.todo.topic_id;
        let todo_id = item.todo.id;
        self.briefing_open = false;
        if let Some(pos) = self.topics.iter().position(|t| t.id == topic_id) {
            self.selected_topic = pos;
            self.reload_todos()?;
            if let Some(todo_pos) = self.todos.iter().position(|t| t.id == todo_id) {
                self.selected_todo = todo_pos;
            }
        }
        self.focus = Focus::Todos;
        Ok(())
    }

    pub fn briefing_open_url(&mut self) {
        let url = self
            .briefing_items
            .get(self.selected_briefing)
            .and_then(|i| i.todo.url.as_deref().map(|s| s.to_string()));
        match url {
            Some(u) => {
                if std::process::Command::new("open").arg(&u).spawn().is_err() {
                    let _ = std::process::Command::new("xdg-open").arg(&u).spawn();
                }
            }
            None => self.status_message = Some("No URL attached to this item".into()),
        }
    }
}

/// Urgency + priority + in_progress bonus for daily briefing ranking.
fn briefing_score(todo: &Todo) -> f32 {
    let today = chrono::Local::now().date_naive();
    let urgency = todo
        .due_date
        .as_deref()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|d| {
            let diff = (d - today).num_days();
            match diff {
                i64::MIN..=-1 => 10.0 + (-diff as f32).min(15.0),
                0 => 10.0,
                1 => 7.0,
                2..=3 => 5.0,
                4..=6 => 3.0,
                7..=13 => 1.0,
                _ => 0.0,
            }
        })
        .unwrap_or(0.0);
    let priority = match todo.priority {
        Some(1) => 8.0,
        Some(2) => 4.0,
        Some(3) => 1.0,
        _ => 0.0,
    };
    urgency + priority + if todo.in_progress { 5.0 } else { 0.0 }
}

/// Maximal Marginal Relevance selection: picks `n` items from `candidates` that
/// balance high focus_score (λ=0.7) against semantic similarity to already-selected
/// items (1-λ=0.3), seeded with the must_do + in_flight embeddings.
fn mmr_select(
    seeds: &[(Todo, String, Option<Vec<f32>>)],
    mut candidates: Vec<(Todo, String, Option<Vec<f32>>)>,
    n: usize,
) -> Vec<(Todo, String, Option<Vec<f32>>)> {
    let max_score = candidates
        .iter()
        .map(|(t, _, _)| briefing_score(t))
        .fold(0.0f32, f32::max)
        .max(1.0);

    let mut selected_embs: Vec<Vec<f32>> = seeds.iter().filter_map(|(_, _, e)| e.clone()).collect();

    let mut result = Vec::new();
    for _ in 0..n {
        if candidates.is_empty() {
            break;
        }
        let best_idx = candidates
            .iter()
            .enumerate()
            .map(|(i, (todo, _, emb))| {
                let score = briefing_score(todo) / max_score;
                let max_sim = emb
                    .as_ref()
                    .map(|e| {
                        selected_embs
                            .iter()
                            .map(|se| cosine_similarity(e, se))
                            .fold(0.0f32, f32::max)
                    })
                    .unwrap_or(0.0);
                (i, 0.7 * score - 0.3 * max_sim)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let chosen = candidates.remove(best_idx);
        if let Some(e) = &chosen.2 {
            selected_embs.push(e.clone());
        }
        result.push(chosen);
    }
    result
}

/// Combine todo text with comment texts for richer embedding.
fn build_embed_text(todo_text: &str, comment_texts: &[String]) -> String {
    if comment_texts.is_empty() {
        todo_text.to_string()
    } else {
        format!("{}\n{}", todo_text, comment_texts.join("\n"))
    }
}

/// Extract !1/!2/!3 priority from text, returning (cleaned_text, priority).
pub fn extract_priority(text: &str) -> (String, Option<u8>) {
    let mut priority = None;
    let cleaned = text
        .split_whitespace()
        .filter(|w| match *w {
            "!1" => {
                priority = Some(1);
                false
            }
            "!2" => {
                priority = Some(2);
                false
            }
            "!3" => {
                priority = Some(3);
                false
            }
            _ => true,
        })
        .collect::<Vec<_>>()
        .join(" ");
    (cleaned, priority)
}

/// Extract the first https:// URL found in a string.
pub fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(|s| {
            s.trim_matches(|c: char| {
                !c.is_alphanumeric()
                    && c != '/'
                    && c != '-'
                    && c != '_'
                    && c != '.'
                    && c != '?'
                    && c != '='
                    && c != '&'
                    && c != '#'
                    && c != ':'
            })
            .to_string()
        })
}
