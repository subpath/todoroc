use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::process::Command;

use crate::db::Database;
use crate::embeddings::Embedder;

#[derive(Debug, Deserialize)]
struct Repo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct PrItem {
    number: u64,
    title: String,
    state: String,
    repository: Repo,
}

pub fn sync(db: &Database, embedder: Option<&Embedder>) -> Result<()> {
    // Remove legacy topic names from earlier versions
    db.delete_topic_by_name("My PRs")?;
    db.delete_topic_by_name("Review Requests")?;
    db.delete_topic_by_name("⎇  My PRs")?;

    println!("Fetching GitHub PRs...\n");

    let my_prs = fetch_prs(&["search", "prs", "--author", "@me", "--state", "open",
        "--json", "number,title,state,repository", "--limit", "50"])
        .context("Failed to fetch your open PRs")?;

    let review_prs = fetch_prs(&["search", "prs", "--review-requested", "@me", "--state", "open",
        "--json", "number,title,state,repository", "--limit", "50"])
        .context("Failed to fetch PRs awaiting your review")?;

    println!("  My open PRs:       {}", my_prs.len());
    println!("  Review requested:  {}", review_prs.len());
    println!();

    sync_topic(db, embedder, "🔀 My PRs", &my_prs)?;
    sync_topic(db, embedder, "👀 Reviews", &review_prs)?;

    println!("\nDone ✓  Open the app to see your todos.");
    Ok(())
}

fn fetch_prs(args: &[&str]) -> Result<Vec<PrItem>> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("Failed to run `gh` — is the GitHub CLI installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh error: {}", stderr.trim());
    }

    let json = String::from_utf8(output.stdout)?;
    let items: Vec<PrItem> = serde_json::from_str(&json)
        .context("Failed to parse gh JSON output")?;
    Ok(items)
}

fn sync_topic(db: &Database, embedder: Option<&Embedder>, topic_name: &str, prs: &[PrItem]) -> Result<()> {
    let topic = db.find_or_create_topic(topic_name)?;

    let pb = ProgressBar::new(prs.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(&format!(
            " {{spinner:.cyan}} {:<20} [{{bar:30.cyan/blue}}] {{pos}}/{{len}}  {{msg}}",
            topic_name
        ))
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏ ")
        .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));

    let mut added = 0usize;
    let mut updated = 0usize;
    let mut closed = 0usize;

    // Track which PR numbers are still open so we can detect merged/closed ones below.
    let open_numbers: std::collections::HashSet<u64> = prs.iter().map(|p| p.number).collect();

    for pr in prs {
        let prefix = format!("#{} ", pr.number);
        let text = format!("#{} {} [{}]", pr.number, pr.title, pr.repository.name_with_owner);
        let done = pr.state.to_lowercase() != "open";

        pb.set_message(format!("#{}", pr.number));

        let url = format!("https://github.com/{}/pull/{}", pr.repository.name_with_owner, pr.number);
        let embedding = embedder.and_then(|e| e.embed(&pr.title).ok());

        match db.find_todo_by_prefix(topic.id, &prefix)? {
            Some((id, _)) => {
                db.update_todo_text_and_done(id, &text, done, Some(url.as_str()), embedding.as_deref())?;
                updated += 1;
            }
            None => {
                db.insert_todo(topic.id, &text, Some(url.as_str()), embedding.as_deref())?;
                added += 1;
            }
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    // Check existing undone todos that were not in the open PR list — they may have been merged/closed.
    for todo in db.todos_for_topic(topic.id)? {
        if todo.done { continue; }
        let pr_number = todo.text
            .strip_prefix('#')
            .and_then(|s| s.split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok());
        let Some(number) = pr_number else { continue };
        if open_numbers.contains(&number) { continue; }

        // PR is no longer open — check its current state via gh.
        let url = todo.url.as_deref().unwrap_or("");
        if url.is_empty() { continue; }
        if !is_pr_open(url) {
            db.update_todo_text_and_done(todo.id, &todo.text, true, Some(url), None)?;
            closed += 1;
        }
    }

    println!("  {} — {} added, {} updated, {} merged/closed", topic_name, added, updated, closed);
    Ok(())
}

/// Returns false if the PR is confirmed merged or closed; true if open or state unknown.
fn is_pr_open(url: &str) -> bool {
    #[derive(Deserialize)]
    struct PrState { state: String }

    let output = Command::new("gh")
        .args(["pr", "view", url, "--json", "state"])
        .output();
    let Ok(output) = output else { return true };
    if !output.status.success() { return true }
    let Ok(json) = String::from_utf8(output.stdout) else { return true };
    let Ok(pr) = serde_json::from_str::<PrState>(&json) else { return true };
    pr.state.to_lowercase() == "open"
}
