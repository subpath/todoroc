use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::db::Database;
use crate::embeddings::Embedder;

fn hf_url(model: &str, file: &str) -> String {
    format!("https://huggingface.co/{}/resolve/main/{}", model, file)
}

pub fn download_model(model_dir: &PathBuf, model: &str) -> Result<()> {
    std::fs::create_dir_all(model_dir)
        .context("Failed to create model directory")?;

    println!("Model: {}", model);
    println!();

    download_file(&hf_url(model, "tokenizer.json"), &model_dir.join("tokenizer.json"), "tokenizer.json")?;
    download_file(&hf_url(model, "onnx/model.onnx"), &model_dir.join("model.onnx"), "model.onnx")?;

    // Save selected model name
    std::fs::write(
        model_dir.parent()
            .context("Model directory has no parent path")?
            .join("model_name.txt"),
        model,
    )?;

    println!();
    println!("Done! Run `todo-tui` to start.");
    Ok(())
}

fn download_file(url: &str, dest: &Path, label: &str) -> Result<()> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("Failed to GET {}", url))?;

    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let pb = if total > 0 {
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::with_template(
                " {msg:20} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        pb.set_message(label.to_string());
        pb
    } else {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template(" {msg:20} {spinner:.cyan} {bytes}")
                .unwrap()
                .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]),
        );
        pb.set_message(label.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        pb
    };

    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("Failed to create {}", dest.display()))?;

    let mut reader = response.into_reader();
    let mut buf = [0u8; 16384];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        file.write_all(&buf[..n])?;
        pb.inc(n as u64);
    }

    pb.finish_with_message(format!("{} ✓", label));
    Ok(())
}

pub fn reindex(db_path: &str, model_dir: &PathBuf) -> Result<()> {
    let db = Database::open(db_path)?;
    let embedder = Embedder::load(model_dir)
        .context("Failed to load embedder — run `todo-tui --model <name>` first")?;

    let todos = db.all_todos()?;
    if todos.is_empty() {
        println!("No todos to index.");
        return Ok(());
    }

    println!("Reindexing {} todos...", todos.len());
    println!();

    let pb = ProgressBar::new(todos.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            " [{bar:40.green/dark_gray}] {pos}/{len}  {msg}"
        )
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏ "),
    );

    let mut errors = 0usize;
    for todo in &todos {
        pb.set_message(truncate(&todo.text, 35));
        let comments = db.all_comment_texts_by_todo(todo.id).unwrap_or_default();
        let embed_text = if comments.is_empty() {
            todo.text.clone()
        } else {
            format!("{}\n{}", todo.text, comments.join("\n"))
        };
        match embedder.embed(&embed_text) {
            Ok(emb) => {
                db.update_embedding(todo.id, &emb)?;
            }
            Err(_) => {
                errors += 1;
            }
        }
        pb.inc(1);
    }

    pb.finish_and_clear();

    if errors > 0 {
        println!("Done — {} embedded, {} failed.", todos.len() - errors, errors);
    } else {
        println!("Done — {} todos indexed ✓", todos.len());
    }
    Ok(())
}

pub fn reindex_headless(db: &Database, embedder: Option<&Embedder>, report: &dyn Fn(&str)) -> Result<()> {
    let Some(embedder) = embedder else {
        report("Reindex: no model loaded, skipping");
        return Ok(());
    };
    let todos = db.all_todos()?;
    report(&format!("Reindexing {} items…", todos.len()));
    for todo in &todos {
        let comments = db.all_comment_texts_by_todo(todo.id).unwrap_or_default();
        let embed_text = if comments.is_empty() {
            todo.text.clone()
        } else {
            format!("{}\n{}", todo.text, comments.join("\n"))
        };
        if let Ok(emb) = embedder.embed(&embed_text) {
            db.update_embedding(todo.id, &emb)?;
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
