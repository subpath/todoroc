use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use crate::db::Database;
use crate::embeddings::Embedder;

#[derive(Debug, Deserialize)]
struct StatusCategory {
    key: String,
}

#[derive(Debug, Deserialize)]
struct Status {
    #[serde(rename = "statusCategory")]
    status_category: StatusCategory,
}

#[derive(Debug, Deserialize)]
struct Fields {
    summary: String,
    status: Status,
}

#[derive(Debug, Deserialize)]
struct DueDateFields {
    #[serde(rename = "duedate")]
    due_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DueDateIssue {
    key: String,
    fields: DueDateFields,
}

#[derive(Debug, Deserialize)]
struct JiraIssue {
    key: String,
    fields: Fields,
}

impl JiraIssue {
    fn browse_url(&self, site_base: &str) -> String {
        format!("{}/browse/{}", site_base, self.key)
    }

    fn is_done(&self) -> bool {
        self.fields.status.status_category.key == "done"
    }
}

/// Read the public Jira site hostname from ~/.config/acli/jira_config.yaml.
fn fetch_site_base_url() -> Option<String> {
    let config_path = dirs::home_dir()?.join(".config/acli/jira_config.yaml");
    let content = std::fs::read_to_string(config_path).ok()?;
    // Find current_profile value to match the right profile, then find its `site:` line.
    // Simple heuristic: take the first `site:` value found (matches the active profile in most setups).
    for line in content.lines() {
        // Matches both `site: host` and `- site: host`
        let trimmed = line.trim().trim_start_matches('-').trim();
        if let Some(rest) = trimmed.strip_prefix("site:") {
            let host = rest.trim().trim_matches('"').trim_matches('\'');
            if !host.is_empty() {
                return Some(format!("https://{}", host));
            }
        }
    }
    None
}

pub fn sync(db: &Database, embedder: Option<&Embedder>) -> Result<()> {
    // Remove legacy single-topic if it exists
    db.delete_topic_by_name("🎫 Jira")?;

    println!("Fetching Jira issues assigned to you...\n");

    let site_base = fetch_site_base_url()
        .context("Could not determine Jira site URL — run `acli jira site list` to verify acli is configured")?;

    let sprint_issues = fetch_issues(
        "assignee = currentUser() AND sprint in openSprints()"
    ).context("Failed to fetch current sprint issues")?;

    let backlog_issues = fetch_issues(
        "assignee = currentUser() AND statusCategory != Done AND (sprint is EMPTY OR sprint not in openSprints())"
    ).context("Failed to fetch backlog issues")?;

    println!("  Current sprint:  {}", sprint_issues.len());
    println!("  Backlog:         {}", backlog_issues.len());
    println!();

    sync_topic(db, embedder, "🎫 Jira Sprint", &sprint_issues, &site_base)?;
    sync_topic(db, embedder, "🗂  Jira Backlog", &backlog_issues, &site_base)?;

    println!("\nDone ✓  Open the app to see your todos.");
    Ok(())
}

fn fetch_issues(jql: &str) -> Result<Vec<JiraIssue>> {
    let output = Command::new("acli")
        .args(["jira", "workitem", "search", "--jql", jql, "--json", "--limit", "50"])
        .output()
        .context("Failed to run `acli` — is the Atlassian CLI installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("acli error: {}", stderr.trim());
    }

    let json = String::from_utf8(output.stdout)?;
    let issues: Vec<JiraIssue> = serde_json::from_str(&json)
        .context("Failed to parse acli JSON output")?;
    Ok(issues)
}

fn fetch_due_dates(issues: &[JiraIssue]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for issue in issues {
        let Ok(output) = Command::new("acli")
            .args(["jira", "workitem", "view", &issue.key, "--fields", "key,duedate", "--json"])
            .output()
        else { continue };
        if !output.status.success() { continue; }
        let Ok(json) = String::from_utf8(output.stdout) else { continue };
        let Ok(parsed) = serde_json::from_str::<DueDateIssue>(&json) else { continue };
        if let Some(due) = parsed.fields.due_date {
            map.insert(parsed.key, due);
        }
    }
    map
}

fn sync_topic(db: &Database, embedder: Option<&Embedder>, topic_name: &str, issues: &[JiraIssue], site_base: &str) -> Result<()> {
    let topic = db.find_or_create_topic(topic_name)?;

    let pb = ProgressBar::new(issues.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            " {spinner:.cyan} {msg:<20} [{bar:30.cyan/blue}] {pos}/{len}"
        )
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏ ")
        .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(80));

    pb.set_message("fetching due dates...");
    let due_dates = fetch_due_dates(issues);

    let mut added = 0usize;
    let mut updated = 0usize;

    for issue in issues {
        pb.set_message(issue.key.clone());

        let text = format!("{} {}", issue.key, issue.fields.summary);
        let url = issue.browse_url(site_base);
        let done = issue.is_done();
        let prefix = format!("{} ", issue.key);
        let embedding = embedder.and_then(|e| e.embed(&issue.fields.summary).ok());

        let todo_id = match db.find_todo_by_prefix(topic.id, &prefix)? {
            Some((id, _)) => {
                db.update_todo_text_and_done(id, &text, done, Some(url.as_str()), embedding.as_deref())?;
                updated += 1;
                id
            }
            None => {
                let todo = db.insert_todo(topic.id, &text, Some(url.as_str()), embedding.as_deref())?;
                added += 1;
                todo.id
            }
        };
        if let Some(due) = due_dates.get(&issue.key) {
            db.set_todo_due_date(todo_id, Some(due.as_str()))?;
        }

        pb.inc(1);
    }

    pb.finish_and_clear();
    println!("  {} — {} added, {} updated", topic_name, added, updated);
    Ok(())
}
