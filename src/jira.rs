use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
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
struct JiraIssue {
    key: String,
    #[serde(rename = "self")]
    self_url: String,
    fields: Fields,
}

impl JiraIssue {
    fn browse_url(&self) -> String {
        // Convert https://host/rest/api/3/issue/ID → https://host/browse/KEY
        let base = self.self_url
            .split("/rest/")
            .next()
            .unwrap_or(&self.self_url);
        format!("{}/browse/{}", base, self.key)
    }

    fn is_done(&self) -> bool {
        self.fields.status.status_category.key == "done"
    }
}

pub fn sync(db: &Database, embedder: Option<&Embedder>) -> Result<()> {
    // Remove legacy single-topic if it exists
    db.delete_topic_by_name("🎫 Jira")?;

    println!("Fetching Jira issues assigned to you...\n");

    let sprint_issues = fetch_issues(
        "assignee = currentUser() AND sprint in openSprints()"
    ).context("Failed to fetch current sprint issues")?;

    let backlog_issues = fetch_issues(
        "assignee = currentUser() AND statusCategory != Done AND (sprint is EMPTY OR sprint not in openSprints())"
    ).context("Failed to fetch backlog issues")?;

    println!("  Current sprint:  {}", sprint_issues.len());
    println!("  Backlog:         {}", backlog_issues.len());
    println!();

    sync_topic(db, embedder, "🎫 Jira Sprint", &sprint_issues)?;
    sync_topic(db, embedder, "🗂  Jira Backlog", &backlog_issues)?;

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

fn sync_topic(db: &Database, embedder: Option<&Embedder>, topic_name: &str, issues: &[JiraIssue]) -> Result<()> {
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

    let mut added = 0usize;
    let mut updated = 0usize;

    for issue in issues {
        pb.set_message(issue.key.clone());

        let text = format!("{} {}", issue.key, issue.fields.summary);
        let url = issue.browse_url();
        let done = issue.is_done();
        let prefix = format!("{} ", issue.key);
        let embedding = embedder.and_then(|e| e.embed(&issue.fields.summary).ok());

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
    println!("  {} — {} added, {} updated", topic_name, added, updated);
    Ok(())
}
