# todoroc

<p align="center">
  <img src="assets/mascot.jpg" width="500" alt="todoroc mascot"/>
</p>

A terminal-based todo manager with semantic search, GitHub, and Jira integrations.

![Rust](https://img.shields.io/badge/rust-stable-orange)

## Features

- **Multi-pane TUI** — topics list and todos list; search is a floating overlay (`/`)
- **Daily briefing** — `D` opens a focused overlay that surfaces your most important todos across all topics, ranked into Must Do / In Flight / Recommended / Waiting sections
- **4-state todos** — `Space` cycles: todo → in progress → blocked → done; blocked items are shown with a `[⊘]` indicator
- **Virtual topics** — built-in "🔄 In Progress", "✅ Completed", and "📅 Due This Week" aggregate views across all topics; toggle visibility with `V`
- **Topic reordering** — `J`/`K` moves topics up and down; order is persisted to the database
- **Cursor memory** — switching topics and coming back restores your last position in each list
- **Detail panel** — press `Enter` to open a full editor: text, priority, due date, URL, timestamps, and comments
- **Comments** — attach threaded comments to any todo from the detail panel
- **Priority levels** — inline `!1`/`!2`/`!3` syntax or `p` key to cycle; color-coded and sorted above other todos
- **Move todos** — `m` key moves the selected todo to any other topic via a popup
- **Background sync** — `S` opens a sync popup (Full / GitHub / Jira); runs in a background thread with a live spinner status
- **Overdue digest** — summary of overdue items printed on launch before opening the TUI
- **Semantic search** — AI-powered search overlay with debounced live results; no cloud needed (local ONNX embeddings)
- **Due dates** — set due dates with natural language input, shown inline with color-coded urgency; `+`/`-` snooze by one day
- **GitHub sync** — pulls your open PRs and pending review requests via `gh` CLI
- **Jira sync** — pulls sprint and backlog items via Atlassian `acli`, including due dates set in Jira
- **Clipboard copy** — `Ctrl+Y` in the detail panel copies the focused field to the clipboard
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
  --compile-model      Compile the ONNX model to an NNEF cache for faster startup
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
| `m` | Move selected todo to another topic |
| `@` | Set due date |
| `+` / `-` | Snooze due date forward / back by one day |
| `Space` | Cycle state: todo → in progress → blocked → done; auto-advances cursor |
| `o` | Open attached URL in browser |
| `s` | Toggle sort: bucketed (priority → due date → creation) / flat |

### Global

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle focus between Topics and Todos |
| `1` / `2` | Focus Topics / Todos |
| `↑↓` / `jk` | Navigate |
| `Shift+↑` / `Shift+↓` | Jump to top / bottom |
| `/` | Open search overlay |
| `D` | Open daily briefing overlay |
| `S` | Open sync popup (Full / GitHub / Jira) |
| `V` | Toggle virtual topics (In Progress / Completed / Due This Week) |
| `n` | New topic (when Topics focused) |
| `e` | Edit selected topic |
| `d` | Delete selected topic |
| `J` / `K` | Move selected topic down / up |
| `i` | Info popup (model, DB stats) |
| `q` | Quit |

### Search overlay

| Key | Action |
|-----|--------|
| *type* | Query — results update live after a short pause |
| `↑↓` | Navigate results |
| `Enter` | Jump to the selected result's topic and todo |
| `o` | Open URL attached to the selected result |
| `Esc` | Close overlay |

### Detail panel

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Move between fields |
| `Enter` | Save (on text/due/URL fields); submit new comment |
| `Ctrl+Y` | Copy current field value to clipboard |
| `Esc` | Close without saving |
| `↑↓` | Scroll detail view |
| `Shift+↑` / `Shift+↓` | Scroll by 5 lines |
| `c` | Jump to new comment field |

## Due Dates

Press `@` on any todo to set a due date. Use `+` / `-` to nudge an existing due date forward or back by one day without opening the popup. Pressing `+` on a todo with no due date sets it to tomorrow.

Supported formats:

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

The first three entries in the topics list are always:

- **🔄 In Progress** — all todos that have been started but not yet completed, across every topic
- **✅ Completed** — all completed todos, across every topic
- **📅 Due This Week** — all unfinished todos due on or before the end of the current ISO week

These are read-only views; new todos cannot be added to them directly.

## Topics

Each topic shows a `[done/total]` count. The count turns green when all items are complete.

Topics can be reordered with `J` (move down) and `K` (move up) while the Topics panel is focused. Order is stored in the database and persists across sessions. Virtual topics (In Progress, Completed, Due This Week) are always pinned at the top and cannot be reordered.

A visual separator divides the virtual topics from your real topics in the list.

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

## Daily Briefing

Press `D` to open the Daily Focus overlay. It pulls together your most important unfinished todos from every topic and organizes them into four ranked sections:

| Section | Contents |
|---------|----------|
| **⚡ Must Do** | High-priority or overdue items |
| **🔄 In Flight** | Items currently in progress |
| **📋 Recommended** | Other actionable items, ranked by urgency and priority |
| **⊘ Waiting** | Blocked items |

Each row shows the todo text, due date badge, priority badge, a link indicator if a URL is attached, and the source topic name in dim text.

| Key | Action |
|-----|--------|
| `↑↓` / `jk` | Navigate items |
| `Space` | Cycle state (todo → in progress → blocked → done) |
| `Enter` | Jump to the item in its topic |
| `+` / `-` | Snooze due date forward / back by one day |
| `o` | Open attached URL in browser |
| `Esc` / `q` | Close overlay |

## Search

Press `/` to open the search overlay. Results update automatically as you type (debounced ~100 ms). Semantic search returns up to 7 unfinished results followed by up to 5 finished results, both sorted by relevance score. Press `Enter` on a result to jump directly to that todo in its topic, or `Esc` to dismiss.

## Integrations

**GitHub** — requires [`gh`](https://cli.github.com/) installed and authenticated. Syncs open PRs and review requests into dedicated topics.

**Jira** — requires [`acli`](https://bobswift.atlassian.net/wiki/spaces/ACLI/overview) installed and authenticated. Syncs sprint and backlog items, including due dates.

Sync can be triggered interactively with `S` inside the TUI. A popup lets you choose Full (GitHub + Jira + reindex), GitHub only, or Jira only. Sync runs in a background thread; a spinner in the bottom-right corner shows progress and turns green on completion.

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

Run `todo --compile-model` once after setup to compile the ONNX model to an NNEF cache (`model.nnef`). Subsequent launches load the cached model and start significantly faster.
