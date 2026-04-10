# todoroc

<p align="center">
  <img src="assets/mascot.jpg" width="500" alt="todoroc mascot"/>
</p>

A terminal-based todo manager with semantic search, GitHub, and Jira integrations.

![Rust](https://img.shields.io/badge/rust-stable-orange)

## Features

- **Multi-pane TUI** — topics list, todos list, and search panel
- **Virtual topics** — built-in "🔄 In Progress" and "✅ Completed" aggregate views across all topics
- **Detail panel** — press `Enter` to open a full editor: text, priority, due date, URL, timestamps, and comments
- **Comments** — attach threaded comments to any todo from the detail panel
- **Priority levels** — inline `!1`/`!2`/`!3` syntax or `p` key to cycle; color-coded and sorted above other todos
- **Overdue digest** — summary of overdue items printed on launch before opening the TUI
- **Semantic search** — AI-powered search across all todos using local ONNX embeddings (no cloud)
- **Due dates** — set due dates with natural language input, shown inline with color-coded urgency
- **GitHub sync** — pulls your open PRs and pending review requests via `gh` CLI
- **Jira sync** — pulls sprint and backlog items via Atlassian `acli`, including due dates set in Jira
- **SQLite storage** — all data stored locally in `~/.todo-tui/todos.db`
- **URL support** — attach and open URLs directly from todos (`o` to open in browser)

## Installation

```bash
# Requires Rust stable
make release

# Install to ~/.local/bin/todo
make install
```

## First Run

```bash
# Download the default embedding model (required for semantic search)
todo --setup

# Launch
todo
```

## Usage

```
todo [OPTIONS]

Options:
  --setup              Download default embedding model
  --model <hf-repo>    Download and activate a Hugging Face ONNX model
  --reindex            Re-embed all todos with the current model
  --clear-db           Delete all data (with confirmation)
  --sync               Full sync: GitHub + Jira + reindex
  --sync-gh            Sync GitHub PRs only
  --sync-jira          Sync Jira issues only
```

## Keybindings

### Todos panel

| Key | Action |
|-----|--------|
| `n` | New todo |
| `Enter` | Open detail panel (edit text, priority, due date, URL, comments) |
| `e` | Edit selected todo inline |
| `p` | Cycle priority: none → `!1` → `!2` → `!3` → none |
| `d` | Delete selected todo |
| `@` | Set due date |
| `Space` | Toggle completion (tracks started/completed timestamps) |
| `o` | Open attached URL in browser |
| `s` | Toggle sort: bucketed (priority → due date → creation) / flat |

### Global

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle focus between panels |
| `1` / `2` / `3` | Focus Topics / Todos / Search |
| `↑↓` / `jk` | Navigate |
| `Shift+↑` / `Shift+↓` | Jump to top / bottom |
| `n` | New topic / search query |
| `e` | Edit selected topic |
| `d` | Delete selected topic |
| `Enter` | Save or execute search |
| `i` | Info popup (model, DB stats) |
| `q` | Quit |

### Detail panel

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Move between fields |
| `Enter` | Save (on text/due/URL fields); submit new comment |
| `Esc` | Close without saving |
| `↑↓` | Scroll detail view |
| `Shift+↑` / `Shift+↓` | Scroll by 5 lines |
| `c` | Jump to new comment field |

## Due Dates

Press `@` on any todo to set a due date. Supported formats:

| Input | Meaning |
|-------|---------|
| `3d` | 3 days from now |
| `fri`, `monday` | Next occurrence of that weekday |
| `next mon` | Explicitly next week's Monday |
| `eow` | End of work week (Friday) |
| `W16` / `w16` / `16w` | ISO work week 16 |
| `2026-04-20` | Absolute date |
| *(empty)* | Clear due date |

Due dates are shown inline before the todo text, color-coded:

- `[⚠ 2d ago]` — red, overdue
- `[today]` — cyan
- `[tmrw]` — yellow
- `[Thu]` — yellow, due this week
- `[Apr 20]` — gray, further out

Jira due dates are pulled automatically on sync.

## Virtual Topics

The first two entries in the topics list are always:

- **🔄 In Progress** — all todos that have been started but not yet completed, across every topic
- **✅ Completed** — all completed todos, across every topic

These are read-only views; new todos cannot be added to them directly.

## Topics

Each topic shows a `[done/total]` count. The count turns green when all items are complete.

## Priority

Todos can have a priority of `!1` (high), `!2` (medium), or `!3` (low).

- **Inline syntax** — type `!1`, `!2`, or `!3` anywhere in the todo text when adding or editing; the tag is stripped and stored separately
- **`p` key** — cycles the priority of the selected todo without opening the editor
- **Sorted first** — in bucketed sort, priority todos appear before due-date-only and unprioritized todos, sorted by priority then due date

## Overdue Digest

On launch, if any todos are overdue, a summary is printed to the terminal before opening the TUI:

```
  ⚠  3 overdue items

  [Work] fix the staging deploy !1  2d ago
  ...

  Press Enter to open the app...
```

## Search

Semantic search returns up to 7 unfinished results followed by up to 5 finished results, both sorted by relevance score.

## Integrations

**GitHub** — requires [`gh`](https://cli.github.com/) installed and authenticated. Syncs open PRs and review requests into dedicated topics.

**Jira** — requires [`acli`](https://bobswift.atlassian.net/wiki/spaces/ACLI/overview) installed and authenticated. Syncs sprint and backlog items, including due dates.

## Development

```bash
make run       # Build and run (debug)
make check     # Fast compile check
make fmt       # Format code
make lint      # Run Clippy
make clean     # Remove build artifacts
```

## Data

| Path | Contents |
|------|----------|
| `~/.todo-tui/todos.db` | SQLite database |
| `~/.todo-tui/model/` | ONNX model + tokenizer |
| `~/.todo-tui/model_name.txt` | Active model name |

The default embedding model is `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`. Any Hugging Face ONNX-compatible model can be used via `--model`.
