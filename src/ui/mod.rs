mod topics;
mod todos;
mod search;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;
use crate::due_date;

pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Split vertically: main area + search bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(9)])
        .split(size);

    let main_area = vertical[0];
    let search_area = vertical[1];

    // Split main area horizontally: topics | todos
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main_area);

    topics::draw(frame, app, horizontal[0]);
    todos::draw(frame, app, horizontal[1]);
    search::draw(frame, app, search_area);

    // Status bar overlay at bottom of topics/todos area
    if let Some(msg) = &app.status_message {
        let status = Paragraph::new(msg.as_str())
            .style(Style::default().fg(Color::Yellow));
        let area = Rect {
            x: main_area.x,
            y: main_area.y + main_area.height.saturating_sub(1),
            width: main_area.width,
            height: 1,
        };
        frame.render_widget(status, area);
    }

    // Info popup
    if app.show_info {
        let (n_topics, n_todos, n_indexed) = app.db.stats().unwrap_or((0, 0, 0));

        let rows: &[(&str, String)] = &[
            ("Model",   app.info.model_name.clone()),
            ("Model dir", app.info.model_dir.clone()),
            ("Database",  app.info.db_path.clone()),
            ("Topics",    n_topics.to_string()),
            ("Items",     n_todos.to_string()),
            ("Indexed",   format!("{} / {}", n_indexed, n_todos)),
        ];

        let label_w = 12u16;
        let dialog_w = 70u16.min(size.width.saturating_sub(4));
        let dialog_h = rows.len() as u16 + 4;
        let x = size.x + (size.width.saturating_sub(dialog_w)) / 2;
        let y = size.y + (size.height.saturating_sub(dialog_h)) / 2;
        let area = Rect { x, y, width: dialog_w, height: dialog_h };

        let block = Block::default()
            .title(Span::styled(" Info ", Style::default().add_modifier(Modifier::BOLD)))
            .title_bottom(Span::styled(
                " any key to close ",
                Style::default().fg(Color::DarkGray),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let mut lines = vec![Line::from("")];
        for (label, value) in rows {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<width$}", label, width = label_w as usize),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(value.clone(), Style::default().fg(Color::White)),
            ]));
        }

        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    // Delete confirmation dialog
    if app.confirm_delete.is_some() {
        let label = app.delete_confirm_label();
        let hint = "[y] yes  [n] no";
        let inner_w = (label.len()).max(hint.len()) as u16 + 4;
        let dialog_w = inner_w.min(size.width.saturating_sub(4));
        let dialog_h = 6u16;
        let x = size.x + (size.width.saturating_sub(dialog_w)) / 2;
        let y = size.y + (size.height.saturating_sub(dialog_h)) / 2;
        let area = Rect { x, y, width: dialog_w, height: dialog_h };

        let block = Block::default()
            .title(Span::styled(" Delete ", Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(label, Style::default().fg(Color::White))),
            Line::from(""),
            Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))),
        ];

        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(text).block(block).alignment(Alignment::Center), area);
    }

    // Due date popup
    if app.due_popup {
        let hint = "Enter:confirm  Esc:cancel  | 3d  fri  eow  W16  16w  YYYY-MM-DD  (empty=clear)";
        let dialog_w = 64u16.min(size.width.saturating_sub(4));
        let dialog_h = 6u16;
        let x = size.x + (size.width.saturating_sub(dialog_w)) / 2;
        let y = size.y + (size.height.saturating_sub(dialog_h)) / 2;
        let area = Rect { x, y, width: dialog_w, height: dialog_h };

        let block = Block::default()
            .title(Span::styled(" Set Due Date ", Style::default().add_modifier(Modifier::BOLD)))
            .title_bottom(Span::styled(
                format!(" {} ", hint),
                Style::default().fg(Color::DarkGray),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let chars: Vec<char> = app.due_input.chars().collect();
        let before: String = chars[..app.due_cursor.min(chars.len())].iter().collect();
        let (cursor_ch, after): (String, String) = if app.due_cursor < chars.len() {
            (chars[app.due_cursor].to_string(), chars[app.due_cursor + 1..].iter().collect())
        } else {
            ("_".to_string(), String::new())
        };

        let input_line = Line::from(vec![
            Span::styled("  Due: ", Style::default().fg(Color::DarkGray)),
            Span::raw(before),
            Span::styled(cursor_ch, Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
            Span::raw(after),
        ]);

        // Preview parsed date
        let preview = match due_date::parse(&app.due_input) {
            Ok(Some(d)) => {
                let (lbl, _) = due_date::label(d);
                format!("  → {}", lbl)
            }
            Ok(None) => "  → (clear)".to_string(),
            Err(e)   => format!("  ✗ {}", e),
        };
        let preview_color = if app.due_input.is_empty() || due_date::parse(&app.due_input).is_ok() {
            Color::DarkGray
        } else {
            Color::Red
        };

        let text = vec![
            Line::from(""),
            input_line,
            Line::from(Span::styled(preview, Style::default().fg(preview_color))),
        ];

        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(text).block(block), area);
    }

    // Quit confirmation dialog
    if app.confirm_quit {
        let dialog_w = 36u16;
        let dialog_h = 5u16;
        let x = size.x + (size.width.saturating_sub(dialog_w)) / 2;
        let y = size.y + (size.height.saturating_sub(dialog_h)) / 2;
        let area = Rect { x, y, width: dialog_w, height: dialog_h };

        let block = Block::default()
            .title(Span::styled(" Quit ", Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Quit todo-tui?  [y] yes  [n] no",
                Style::default().fg(Color::White),
            )),
        ];

        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(text).block(block).alignment(Alignment::Center), area);
    }
}

pub fn focused_block(title: &str, focused: bool) -> Block<'_> {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style)
}
