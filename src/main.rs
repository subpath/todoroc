mod app;
mod db;
mod due_date;
mod embeddings;
mod github;
mod jira;
mod models;
mod setup;
mod sync;
mod ui;

use std::{
    fs,
    io,
    path::PathBuf,
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::{App, AppInfo, DetailField, Focus, Mode};
use std::time::Instant;
use sync::SyncKind;
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

    // Overdue digest
    show_overdue_digest(&app)?;

    // Restore terminal on panic so the error is actually readable
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
        );
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn show_overdue_digest(app: &App) -> Result<()> {
    let overdue = app.db.overdue_todos()?;
    if overdue.is_empty() {
        return Ok(());
    }

    let today = chrono::Local::now().date_naive();
    println!("\n  ⚠  {} overdue item{}\n", overdue.len(), if overdue.len() == 1 { "" } else { "s" });
    for (todo, topic) in &overdue {
        let days_ago = todo.due_date.as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .map(|d| (today - d).num_days())
            .unwrap_or(0);
        let pri = match todo.priority {
            Some(1) => " !1",
            Some(2) => " !2",
            Some(3) => " !3",
            _       => "",
        };
        println!("  {}d ago{}  [{}]  {}", days_ago, pri, topic, todo.text);
    }
    println!("\n  Press Enter to open the app...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        app.poll_sync()?;

        // Fire debounced search ~250 ms after the last query keystroke
        if let Some(t) = app.search_debounce {
            if t.elapsed() >= Duration::from_millis(100) {
                app.search_debounce = None;
                app.run_search()?;
            }
        }

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
            } else if app.sync_popup {
                handle_sync_popup(app, key.code)?;
            } else if app.due_popup {
                handle_due_popup(app, key.code)?;
            } else if app.move_popup {
                handle_move_popup(app, key.code)?;
            } else if app.detail.is_some() {
                handle_detail(app, key.code, key.modifiers)?;
            } else if app.briefing_open {
                handle_briefing(app, key.code)?;
            } else if app.search_open {
                handle_search_overlay(app, key.code)?;
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

fn handle_search_overlay(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc => {
            app.search_open = false;
        }
        KeyCode::Enter => {
            if !app.search_results.is_empty() {
                app.search_open = false;
                app.jump_to_search_result()?;
            } else if !app.search_query.is_empty() {
                app.search_debounce = None;
                app.run_search()?;
            }
        }
        KeyCode::Up => {
            if app.selected_search_result > 0 {
                app.selected_search_result -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_search_result + 1 < app.search_results.len() {
                app.selected_search_result += 1;
            }
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            if app.search_query.is_empty() {
                app.search_results.clear();
                app.selected_search_result = 0;
                app.search_debounce = None;
            } else {
                app.search_debounce = Some(Instant::now());
            }
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.search_debounce = Some(Instant::now());
        }
        _ => {}
    }
    Ok(())
}

fn handle_normal(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match key {
        KeyCode::Char('q') => app.confirm_quit = true,
        KeyCode::Char('i') => app.show_info = true,

        KeyCode::Char('1') => { app.focus = Focus::Topics; app.mode = Mode::Normal; }
        KeyCode::Char('2') => { app.focus = Focus::Todos; app.mode = Mode::Normal; }

        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Topics => Focus::Todos,
                Focus::Todos  => Focus::Topics,
            };
        }
        KeyCode::BackTab => {
            app.focus = match app.focus {
                Focus::Topics => Focus::Todos,
                Focus::Todos  => Focus::Topics,
            };
        }

        // Navigation
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => app.nav_top(),
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => app.nav_bottom(),
        KeyCode::Up | KeyCode::Char('k') => app.nav_up(),
        KeyCode::Down | KeyCode::Char('j') => app.nav_down(),
        KeyCode::Left  => { if app.focus == Focus::Todos  { app.focus = Focus::Topics; } }
        KeyCode::Right => { if app.focus == Focus::Topics { app.focus = Focus::Todos;  } }

        // / opens the search overlay
        KeyCode::Char('/') => {
            app.search_open = true;
            app.search_query.clear();
            app.search_results.clear();
            app.selected_search_result = 0;
            app.search_debounce = None;
        }

        // Actions — n enters insert mode
        KeyCode::Char('n') => {
            if app.focus == Focus::Todos && app.is_virtual_topic() {
                app.status_message = Some("Cannot add todos to virtual topics".into());
            } else {
                app.mode = Mode::Insert;
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
            }
        }

        KeyCode::Char('d') => match app.focus {
            Focus::Topics if !app.topics.is_empty() && !app.is_virtual_topic() => {
                app.confirm_delete = Some(Focus::Topics);
            }
            Focus::Todos if !app.todos.is_empty() && !app.is_virtual_topic() => {
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
            if app.focus == Focus::Todos && !app.todos.is_empty() {
                app.open_detail();
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

        KeyCode::Char('+') => {
            if app.focus == Focus::Todos && !app.todos.is_empty() {
                app.snooze_due_date(1)?;
            }
        }

        KeyCode::Char('-') => {
            if app.focus == Focus::Todos && !app.todos.is_empty() {
                app.snooze_due_date(-1)?;
            }
        }

        KeyCode::Char('p') => {
            if app.focus == Focus::Todos {
                app.cycle_priority()?;
            }
        }

        KeyCode::Char('m') => {
            if app.focus == Focus::Todos && !app.todos.is_empty() {
                app.open_move_popup();
            }
        }

        KeyCode::Char('J') => {
            if app.focus == Focus::Topics {
                app.move_topic_down()?;
            }
        }

        KeyCode::Char('K') => {
            if app.focus == Focus::Topics {
                app.move_topic_up()?;
            }
        }

        KeyCode::Char('S') => app.open_sync_popup(),

        KeyCode::Char('D') => app.open_briefing()?,

        KeyCode::Char('V') => app.toggle_virtual_topics()?,

        _ => {}
    }
    Ok(())
}

fn handle_briefing(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc | KeyCode::Char('q') => app.close_briefing(),
        KeyCode::Up   | KeyCode::Char('k') => {
            if app.selected_briefing > 0 { app.selected_briefing -= 1; }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.selected_briefing + 1 < app.briefing_items.len() {
                app.selected_briefing += 1;
            }
        }
        KeyCode::Enter => app.briefing_jump()?,
        KeyCode::Char(' ') => app.briefing_toggle_todo()?,
        KeyCode::Char('+') => app.briefing_snooze(1)?,
        KeyCode::Char('-') => app.briefing_snooze(-1)?,
        KeyCode::Char('o') => app.briefing_open_url(),
        _ => {}
    }
    Ok(())
}

fn field_scroll_target(field: &DetailField) -> u16 {
    match field {
        DetailField::Text               => 0,
        DetailField::Priority           => 3,
        DetailField::Due                => 5,
        DetailField::Url                => 8,
        DetailField::NewComment         => 11,
        DetailField::ExistingComment(i) => 14 + (*i as u16) * 4,
    }
}

fn handle_detail(app: &mut App, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    let Some(d) = app.detail.as_ref() else { return Ok(()); };

    match key {
        KeyCode::Esc => { app.close_detail(); return Ok(()); }
        KeyCode::Enter => {
            match &d.field.clone() {
                DetailField::NewComment => {
                    app.save_new_comment()?;
                    return Ok(());
                }
                DetailField::ExistingComment(_) => {
                    // Enter saves edits (same as tab-away)
                    app.save_comment_edit()?;
                    return Ok(());
                }
                _ => { app.confirm_detail()?; return Ok(()); }
            }
        }
        KeyCode::Tab => {
            let Some(d) = app.detail.as_ref() else { return Ok(()); };
            let next_field = d.field.next(d.comments.len());
            if matches!(d.field, DetailField::ExistingComment(_)) {
                app.save_comment_edit()?;
            }
            let Some(d) = app.detail.as_mut() else { return Ok(()); };
            d.field = next_field.clone();
            d.detail_scroll = field_scroll_target(&next_field);
            if let DetailField::ExistingComment(i) = &next_field {
                app.enter_comment_edit(*i);
            }
            return Ok(());
        }
        KeyCode::BackTab => {
            let Some(d) = app.detail.as_ref() else { return Ok(()); };
            let prev_field = d.field.prev(d.comments.len());
            if matches!(d.field, DetailField::ExistingComment(_)) {
                app.save_comment_edit()?;
            }
            let Some(d) = app.detail.as_mut() else { return Ok(()); };
            d.field = prev_field.clone();
            d.detail_scroll = field_scroll_target(&prev_field);
            if let DetailField::ExistingComment(i) = &prev_field {
                app.enter_comment_edit(*i);
            }
            return Ok(());
        }
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(d) = app.detail.as_mut() {
                d.detail_scroll = d.detail_scroll.saturating_sub(5);
            }
            return Ok(());
        }
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(d) = app.detail.as_mut() {
                d.detail_scroll = d.detail_scroll.saturating_add(5);
            }
            return Ok(());
        }
        KeyCode::Up => {
            if let Some(d) = app.detail.as_mut() {
                d.detail_scroll = d.detail_scroll.saturating_sub(1);
            }
            return Ok(());
        }
        KeyCode::Down => {
            if let Some(d) = app.detail.as_mut() {
                d.detail_scroll = d.detail_scroll.saturating_add(1);
            }
            return Ok(());
        }
        KeyCode::Char('y') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(d) = app.detail.as_ref() {
                let text = match &d.field {
                    DetailField::Text => d.text.clone(),
                    DetailField::Priority => match d.priority {
                        Some(1) => "High".to_string(),
                        Some(2) => "Medium".to_string(),
                        Some(3) => "Low".to_string(),
                        _ => String::new(),
                    },
                    DetailField::Due => d.due.clone(),
                    DetailField::Url => d.url.clone(),
                    DetailField::NewComment => d.new_comment.clone(),
                    DetailField::ExistingComment(_) => d.comment_edit_text.clone(),
                };
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
            return Ok(());
        }
        KeyCode::Char('c') => {
            if matches!(app.detail.as_ref().map(|d| &d.field), Some(DetailField::NewComment)) {
                // already there — do nothing, fall through to edit
            } else {
                if let Some(d) = app.detail.as_mut() {
                    if matches!(d.field, DetailField::ExistingComment(_)) {
                        app.save_comment_edit()?;
                    }
                    if let Some(d) = app.detail.as_mut() {
                        d.field = DetailField::NewComment;
                        d.detail_scroll = field_scroll_target(&DetailField::NewComment);
                    }
                }
                return Ok(());
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let Some(d) = app.detail.as_ref() else { return Ok(()); };
            if matches!(d.field, DetailField::ExistingComment(_)) {
                app.delete_selected_comment()?;
                return Ok(());
            }
        }
        _ => {}
    }

    let Some(d) = app.detail.as_mut() else { return Ok(()); };
    match d.field.clone() {
        DetailField::Priority => match key {
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                d.priority = match d.priority {
                    None     => Some(1),
                    Some(1)  => Some(2),
                    Some(2)  => Some(3),
                    _        => None,
                };
            }
            _ => {}
        },
        DetailField::Text => edit_field(&mut d.text, &mut d.text_cursor, key),
        DetailField::Due  => edit_field(&mut d.due,  &mut d.due_cursor,  key),
        DetailField::Url  => edit_field(&mut d.url,  &mut d.url_cursor,  key),
        DetailField::NewComment => edit_field(&mut d.new_comment, &mut d.new_comment_cursor, key),
        DetailField::ExistingComment(_) => edit_field(&mut d.comment_edit_text, &mut d.comment_edit_cursor, key),
    }
    Ok(())
}

fn edit_field(text: &mut String, cursor: &mut usize, key: KeyCode) {
    match key {
        KeyCode::Backspace => { if delete_char_before(text, *cursor) { *cursor -= 1; } }
        KeyCode::Left  => { if *cursor > 0 { *cursor -= 1; } }
        KeyCode::Right => { if *cursor < text.chars().count() { *cursor += 1; } }
        KeyCode::Char(c) => { insert_char_at(text, *cursor, c); *cursor += 1; }
        _ => {}
    }
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

fn handle_sync_popup(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc => app.close_sync_popup(),
        KeyCode::Enter => {
            let kind = match app.sync_popup_selected {
                0 => SyncKind::Full,
                1 => SyncKind::GitHub,
                _ => SyncKind::Jira,
            };
            app.start_sync(kind);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.sync_popup_selected > 0 { app.sync_popup_selected -= 1; }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.sync_popup_selected < 2 { app.sync_popup_selected += 1; }
        }
        _ => {}
    }
    Ok(())
}

fn handle_move_popup(app: &mut App, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc => app.close_move_popup(),
        KeyCode::Enter => app.confirm_move_todo()?,
        KeyCode::Up | KeyCode::Char('k') => {
            if app.move_popup_selected > 0 {
                app.move_popup_selected -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let count = app.move_popup_topics().len();
            if app.move_popup_selected + 1 < count {
                app.move_popup_selected += 1;
            }
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
            let text = app.input.clone();
            if !text.is_empty() {
                match app.focus {
                    Focus::Topics => {
                        if app.editing { app.update_topic(&text)?; }
                        else           { app.add_topic(&text)?; }
                    }
                    Focus::Todos => {
                        if app.editing { app.update_todo(&text)?; }
                        else           { app.add_todo(&text)?; }
                    }
                }
            }
            app.mode = Mode::Normal;
            app.input.clear();
            app.cursor_pos = 0;
            app.editing = false;
        }

        KeyCode::Backspace => {
            if delete_char_before(&mut app.input, app.cursor_pos) {
                app.cursor_pos -= 1;
            }
        }

        KeyCode::Left => {
            if app.cursor_pos > 0 { app.cursor_pos -= 1; }
        }

        KeyCode::Right => {
            if app.cursor_pos < app.input.chars().count() { app.cursor_pos += 1; }
        }

        KeyCode::Char(c) => {
            insert_char_at(&mut app.input, app.cursor_pos, c);
            app.cursor_pos += 1;
        }

        _ => {}
    }
    Ok(())
}
