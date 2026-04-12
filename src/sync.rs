use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::db::Database;
use crate::embeddings::Embedder;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncKind {
    Full,
    GitHub,
    Jira,
}

impl SyncKind {
    pub fn label(self) -> &'static str {
        match self {
            SyncKind::Full => "Full  (GitHub + Jira)",
            SyncKind::GitHub => "GitHub  (PRs & reviews)",
            SyncKind::Jira => "Jira  (sprint & backlog)",
        }
    }
}

pub enum SyncMsg {
    Status(String),
    Done,
    Error(String),
}

pub fn start(kind: SyncKind, db_path: String, model_dir: PathBuf) -> mpsc::Receiver<SyncMsg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || match run(kind, &db_path, &model_dir, &tx) {
        Ok(()) => {
            let _ = tx.send(SyncMsg::Done);
        }
        Err(e) => {
            let _ = tx.send(SyncMsg::Error(e.to_string()));
        }
    });
    rx
}

fn run(
    kind: SyncKind,
    db_path: &str,
    model_dir: &Path,
    tx: &mpsc::Sender<SyncMsg>,
) -> Result<()> {
    let db = Database::open(db_path)?;
    let embedder = if model_dir.exists() {
        Embedder::load(model_dir).ok()
    } else {
        None
    };
    let report = |msg: &str| {
        let _ = tx.send(SyncMsg::Status(msg.to_string()));
    };

    if matches!(kind, SyncKind::Full | SyncKind::GitHub) {
        crate::github::sync_headless(&db, embedder.as_ref(), &report)?;
    }
    if matches!(kind, SyncKind::Full | SyncKind::Jira) {
        crate::jira::sync_headless(&db, embedder.as_ref(), &report)?;
    }
    crate::setup::reindex_headless(&db, embedder.as_ref(), &report)?;
    Ok(())
}
