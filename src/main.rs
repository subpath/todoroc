mod app;
mod db;
mod due_date;
mod embeddings;
mod github;
mod jira;
mod models;
mod setup;
mod ui;

use std::{
    fs,
    io,
    path::PathBuf,
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::{App, AppInfo, Focus, Mode};
use db::Database;
use embeddings::Embedder;

fn data_dir() -> PathBuf {
    let dir = dirs_home().join(".todo-tui");
    fs::create_dir_all(&dir).ok();
    dir
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = data_dir();

    // --model <hf-repo>  →  download and activate model
    if let Some(pos) = args.iter().position(|a| a == "--model") {
        let model = args.get(pos + 1)
            .map(|s| s.as_str())
            .unwrap_or("sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2");
        setup::download_model(&dir.join("model"), model)?;
        return Ok(());
    }

    // --setup  →  download default model (kept for backwards compat)
    if args.iter().any(|a| a == "--setup") {
        setup::download_model(&dir.join("model"), "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2")?;
        return Ok(());
    }

    // --reindex  →  re-embed all todos with current model
    if args.iter().any(|a| a == "--reindex") {
        let db_path = dir.join("todos.db");
        setup::reindex(db_path.to_str().unwrap(), &dir.join("model"))?;
        return Ok(());
    }

    // --clear-db  →  delete all topics and todos
    if args.iter().any(|a| a == "--clear-db") {
        let db_path = dir.join("todos.db");
        print!("This will delete ALL topics and todos. Are you sure? [y/N] ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input)?;
        if input.trim().to_lowercase() == "y" {
            let db = Database::open(db_path.to_str().unwrap())?;
            db.clear()?;
            println!("Database cleared.");
        } else {
            println!("Aborted.");
        }
        return Ok(());
    }

    // --sync  →  full sync: gh + jira + reindex
    if args.iter().any(|a| a == "--sync") {
        let db_path = dir.join("todos.db");
        let model_dir = dir.join("model");
        let db = Database::open(db_path.to_str().unwrap())?;
        let embedder = if model_dir.exists() { Embedder::load(&model_dir).ok() } else { None };

        println!("━━━ GitHub ━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        github::sync(&db, embedder.as_ref())?;
        println!();
        println!("━━━ Jira ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        jira::sync(&db, embedder.as_ref())?;
        println!();
        println!("━━━ Reindex ━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        setup::reindex(db_path.to_str().unwrap(), &model_dir)?;
        return Ok(());
    }

    // --sync-jira  →  pull assigned Jira issues into a todo topic
    if args.iter().any(|a| a == "--sync-jira") {
        let db_path = dir.join("todos.db");
        let db = Database::open(db_path.to_str().unwrap())?;
        let model_dir = dir.join("model");
        let embedder = if model_dir.exists() { Embedder::load(&model_dir).ok() } else { None };
        jira::sync(&db, embedder.as_ref())?;
        return Ok(());
    }

    // --sync-gh  →  pull open PRs from GitHub into todo topics
    if args.iter().any(|a| a == "--sync-gh") {
        let db_path = dir.join("todos.db");
        let db = Database::open(db_path.to_str().unwrap())?;
        let model_dir = dir.join("model");
        let embedder = if model_dir.exists() {
            Embedder::load(&model_dir).ok()
        } else {
            None
        };
        github::sync(&db, embedder.as_ref())?;
        return Ok(());
    }

    let dir = data_dir();
    let db_path = dir.join("todos.db");
    let db = Database::open(db_path.to_str().unwrap())?;

    // Try to load embedder; if model files missing, run without search
    let model_dir = dir.join("model");
    let model_name = std::fs::read_to_string(dir.join("model_name.txt"))
        .unwrap_or_else(|_| if model_dir.exists() {
            "unknown (run --model to set)".into()
        } else {
            "none".into()
        })
        .trim()
        .to_string();

    let embedder = if model_dir.exists() {
        match Embedder::load(&model_dir) {
            Ok(e) => Some(e),
            Err(err) => {
                eprintln!("Warning: could not load embedder: {err}");
                None
            }
        }
    } else {
        None
    };

    let info = AppInfo {
        db_path: db_path.display().to_string(),
        model_dir: model_dir.display().to_string(),
        model_name,
    };

    let mut app = App::new(db, embedder, info)?;
    if app.embedder.is_none() {
        app.status_message = Some(
            "No model — run `todo-tui --model sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`".into(),
        );
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            app.status_message = None;

            if app.confirm_quit {
                handle_confirm_quit(app, key.code);
            } else if app.confirm_delete.is_some() {
                handle_confirm_delete(app, key.code)?;
            } else if app.show_info {
                app.show_info = false;
            } else if app.due_popup {
                handle_due_popup(app, key.code)?;
            } else {
                match app.mode {
                    Mode::Normal => handle_normal(app, key.code, key.modifiers)?,
                    Mode::Insert => handle_insert(app, key.code)?,
                }
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_confirm_delete(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Char('y') | KeyCode::Enter => {
            match app.confirm_delete.take() {
                Some(Focus::Topics) => app.delete_topic()?,
                Some(Focus::Todos)  => app.delete_todo()?,
                _ => {}
            }
        }
        _ => { app.confirm_delete = None; }
    }
    Ok(())
}

fn handle_confirm_quit(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('y') | KeyCode::Enter => app.should_quit = true,
        _ => app.confirm_quit = false,
    }
}

fn insert_char_at(s: &mut String, pos: usize, c: char) {
    let byte_pos = s.char_indices().nth(pos).map(|(i, _)| i).unwrap_or(s.len());
    s.insert(byte_pos, c);
}

fn delete_char_before(s: &mut String, pos: usize) -> bool {
    if pos == 0 { return false; }
    if let Some((byte_pos, _)) = s.char_indices().nth(pos - 1) {
        s.remove(byte_pos);
        true
    } else {
        false
    }
}

fn handle_normal(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) -> Result<()> {
    match key {
        KeyCode::Char('q') => app.confirm_quit = true,
        KeyCode::Char('i') => app.show_info = true,

        KeyCode::Char('1') => { app.focus = Focus::Topics; app.mode = Mode::Normal; }
        KeyCode::Char('2') => { app.focus = Focus::Todos; app.mode = Mode::Normal; }
        KeyCode::Char('3') => app.focus = Focus::Search,

        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Topics => Focus::Todos,
                Focus::Todos => Focus::Search,
                Focus::Search => Focus::Topics,
            };
        }
        KeyCode::BackTab => {
            app.focus = match app.focus {
                Focus::Topics => Focus::Search,
                Focus::Todos => Focus::Topics,
                Focus::Search => Focus::Todos,
            };
        }

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => app.nav_up(),
        KeyCode::Down | KeyCode::Char('j') => app.nav_down(),
        KeyCode::Left  => { app.focus = match app.focus { Focus::Todos | Focus::Search => Focus::Topics, f => f }; }
        KeyCode::Right => { app.focus = match app.focus { Focus::Topics | Focus::Search => Focus::Todos,  f => f }; }

        // Actions — n enters insert in all panels
        KeyCode::Char('n') => {
            app.mode = Mode::Insert;
            if app.focus != Focus::Search {
                app.input.clear();
                app.cursor_pos = 0;
                app.editing = false;
            }
        }

        KeyCode::Char('e') => {
            match app.focus {
                Focus::Topics => {
                    if let Some(topic) = app.topics.get(app.selected_topic) {
                        app.input = topic.name.clone();
                        app.cursor_pos = app.input.chars().count();
                        app.editing = true;
                        app.mode = Mode::Insert;
                    }
                }
                Focus::Todos => {
                    if let Some(todo) = app.todos.get(app.selected_todo) {
                        app.input = todo.text.clone();
                        app.cursor_pos = app.input.chars().count();
                        app.editing = true;
                        app.mode = Mode::Insert;
                    }
                }
                _ => {}
            }
        }

        KeyCode::Char('d') => match app.focus {
            Focus::Topics if !app.topics.is_empty() => {
                app.confirm_delete = Some(Focus::Topics);
            }
            Focus::Todos if !app.todos.is_empty() => {
                app.confirm_delete = Some(Focus::Todos);
            }
            _ => {}
        },

        KeyCode::Char(' ') => {
            if app.focus == Focus::Todos {
                app.toggle_todo()?;
            }
        }

        KeyCode::Enter => {
            if app.focus == Focus::Search && !app.search_results.is_empty() {
                app.jump_to_search_result()?;
            }
        }

        KeyCode::Char('o') => app.open_url(),

        KeyCode::Char('s') => {
            if app.focus == Focus::Todos {
                app.toggle_todo_sort()?;
            }
        }

        KeyCode::Char('@') => {
            if app.focus == Focus::Todos {
                app.open_due_popup();
            }
        }

        _ => {}
    }
    Ok(())
}

fn handle_due_popup(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc => app.close_due_popup(),
        KeyCode::Enter => app.confirm_due_date()?,
        KeyCode::Backspace => {
            if delete_char_before(&mut app.due_input, app.due_cursor) {
                app.due_cursor -= 1;
            }
        }
        KeyCode::Left => {
            if app.due_cursor > 0 { app.due_cursor -= 1; }
        }
        KeyCode::Right => {
            if app.due_cursor < app.due_input.chars().count() { app.due_cursor += 1; }
        }
        KeyCode::Char(c) => {
            insert_char_at(&mut app.due_input, app.due_cursor, c);
            app.due_cursor += 1;
        }
        _ => {}
    }
    Ok(())
}

fn handle_insert(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input.clear();
            app.cursor_pos = 0;
            app.editing = false;
        }

        KeyCode::Enter => {
            let text = if app.focus == Focus::Search {
                app.search_query.clone()
            } else {
                app.input.clone()
            };

            if !text.is_empty() {
                match app.focus {
                    Focus::Topics => {
                        if app.editing {
                            app.update_topic(&text)?;
                        } else {
                            app.add_topic(&text)?;
                        }
                    }
                    Focus::Todos => {
                        if app.editing {
                            app.update_todo(&text)?;
                        } else {
                            app.add_todo(&text)?;
                        }
                    }
                    Focus::Search => {
                        app.run_search()?;
                    }
                }
            }

            if app.focus == Focus::Search {
                app.mode = Mode::Normal; // switch to normal so arrows navigate results
            } else {
                app.mode = Mode::Normal;
                app.input.clear();
                app.cursor_pos = 0;
                app.editing = false;
            }
        }

        KeyCode::Backspace => {
            if app.focus == Focus::Search {
                app.search_query.pop();
            } else if delete_char_before(&mut app.input, app.cursor_pos) {
                app.cursor_pos -= 1;
            }
        }

        KeyCode::Left => {
            if app.focus != Focus::Search && app.cursor_pos > 0 {
                app.cursor_pos -= 1;
            }
        }

        KeyCode::Right => {
            if app.focus != Focus::Search && app.cursor_pos < app.input.chars().count() {
                app.cursor_pos += 1;
            }
        }

        KeyCode::Char(c) => {
            if app.focus == Focus::Search {
                app.search_query.push(c);
            } else {
                insert_char_at(&mut app.input, app.cursor_pos, c);
                app.cursor_pos += 1;
            }
        }

        _ => {}
    }
    Ok(())
}
