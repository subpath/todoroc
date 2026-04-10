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

use crate::app::{App, DetailField};
use crate::due_date;
use chrono;

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

/// Render a 2-row wrapping text area with inline cursor. Returns the row Lines.
/// `label_fn(row)` returns the label/indent span for each row.
fn render_multiline_field(
    text: &str,
    cursor: usize,
    active: bool,
    rows: usize,
    field_w: usize,
    label_fn: impl Fn(usize) -> Span<'static>,
) -> Vec<Line<'static>> {
    let text_style  = Style::default().fg(if active { Color::White } else { Color::Gray });
    let cursor_style = Style::default().fg(Color::Black).bg(Color::Cyan);
    let chars: Vec<char> = text.chars().collect();
    let cursor_line = if field_w > 0 { cursor / field_w } else { 0 };
    let scroll_top  = cursor_line.saturating_sub(rows - 1);
    let mut out = Vec::new();
    for row in 0..rows {
        let vis_line = scroll_top + row;
        let start = vis_line * field_w;
        let end   = (start + field_w).min(chars.len());
        let label = label_fn(row);
        if start > chars.len() {
            let cur = if active && cursor == chars.len() && vis_line == cursor_line {
                Span::styled("_", cursor_style)
            } else {
                Span::raw("")
            };
            out.push(Line::from(vec![label, cur]));
            continue;
        }
        let line_chars = &chars[start..end];
        let cursor_col = if active && cursor >= start && cursor < start + field_w {
            Some(cursor - start)
        } else if active && cursor == chars.len() && vis_line == cursor_line {
            Some(end - start)
        } else {
            None
        };
        let mut spans = vec![label];
        for (i, ch) in line_chars.iter().enumerate() {
            spans.push(if cursor_col == Some(i) {
                Span::styled(ch.to_string(), cursor_style)
            } else {
                Span::styled(ch.to_string(), text_style)
            });
        }
        if cursor_col == Some(line_chars.len()) {
            spans.push(Span::styled("_", cursor_style));
        }
        out.push(Line::from(spans));
    }
    out
}

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

    // Detail popup
    if let Some(d) = &app.detail {
        let dialog_w = size.width.saturating_sub(6).min(90);
        let dialog_h = size.height.saturating_sub(4).min(26);
        let x = size.x + (size.width.saturating_sub(dialog_w)) / 2;
        let y = size.y + (size.height.saturating_sub(dialog_h)) / 2;
        let area = Rect { x, y, width: dialog_w, height: dialog_h };

        let block = Block::default()
            .title(Span::styled(" Item Detail ", Style::default().add_modifier(Modifier::BOLD)))
            .title_bottom(Span::styled(
                " Tab:field  ↑↓:scroll  c:comment  Enter:save  Esc:cancel ",
                Style::default().fg(Color::DarkGray),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let label_w: usize = 10;
        let indent: usize = 2;
        let field_w = (dialog_w as usize).saturating_sub(label_w + indent + 4);

        // Render a single-line editable field
        let render_line = |label: &str, value: &str, cursor: usize, active: bool| -> Line {
            let chars: Vec<char> = value.chars().collect();
            let (cur_ch, rest): (String, String) = if cursor < chars.len() {
                (chars[cursor].to_string(), chars[cursor + 1..].iter().collect())
            } else {
                ("_".to_string(), String::new())
            };
            let before: String = chars[..cursor].iter().collect();
            // scroll before-text so cursor is always visible
            let before_disp: String = before.chars().rev().take(field_w).collect::<Vec<_>>().into_iter().rev().collect();
            let text_style  = Style::default().fg(if active { Color::White } else { Color::Gray });
            let cursor_style = Style::default().fg(Color::Black).bg(Color::Cyan);
            let label_style = Style::default().fg(if active { Color::Cyan } else { Color::DarkGray });
            Line::from(vec![
                Span::styled(format!("{:indent$}{:<lw$}  ", "", label, indent = indent, lw = label_w), label_style),
                Span::styled(before_disp, text_style),
                if active { Span::styled(cur_ch.clone(), cursor_style) } else { Span::styled(cur_ch.clone(), text_style) },
                Span::styled(rest, text_style),
            ])
        };

        let text_active = d.field == DetailField::Text;
        let text_label_style = Style::default().fg(if text_active { Color::Cyan } else { Color::DarkGray });
        let text_lines = render_multiline_field(&d.text, d.text_cursor, text_active, 2, field_w, move |row| {
            if row == 0 {
                Span::styled(format!("{:indent$}{:<lw$}  ", "", "Text", indent = indent, lw = label_w), text_label_style)
            } else {
                Span::styled(format!("{:width$}", "", width = indent + label_w + 2), Style::default())
            }
        });

        let mut lines: Vec<Line> = vec![Line::from("")];
        lines.extend(text_lines);
        lines.push(Line::from(""));

        // Priority
        let pri_active = d.field == DetailField::Priority;
        let (pri_dot, pri_text, pri_color) = match d.priority {
            Some(1) => ("[!!] ", "High",   Color::Red),
            Some(2) => ("[!]  ", "Medium", Color::Yellow),
            Some(3) => ("[.]  ", "Low",    Color::Blue),
            _       => ("[  ] ", "None",   Color::DarkGray),
        };
        let hint_txt = "  ←/→ to change";
        lines.push(Line::from(vec![
            Span::styled(format!("{:indent$}{:<lw$}  ", "", "Priority", indent = indent, lw = label_w),
                Style::default().fg(if pri_active { Color::Cyan } else { Color::DarkGray })),
            Span::styled(pri_dot, Style::default().fg(pri_color)),
            Span::styled(pri_text, Style::default().fg(if pri_active { Color::White } else { Color::Gray })),
            Span::styled(if pri_active { hint_txt } else { "" }, Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));

        // Due date
        let due_active = d.field == DetailField::Due;
        lines.push(render_line("Due", &d.due, d.due_cursor, due_active));
        if !d.due.is_empty() {
            let (preview, ok) = match due_date::parse(&d.due) {
                Ok(Some(date)) => { let (lbl, _) = due_date::label(date); (format!("{:indent$}{:<lw$}  → {}", "", "", lbl, indent=indent, lw=label_w), true) }
                Ok(None)       => (format!("{:indent$}{:<lw$}  → (clear)", "", "", indent=indent, lw=label_w), true),
                Err(e)         => (format!("{:indent$}{:<lw$}  ✗ {}", "", "", e, indent=indent, lw=label_w), false),
            };
            lines.push(Line::from(Span::styled(preview, Style::default().fg(if ok { Color::DarkGray } else { Color::Red }))));
        } else {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(""));

        // URL — 2 lines, always shown from the beginning
        let url_active = d.field == DetailField::Url;
        let url_style  = Style::default().fg(if url_active { Color::White } else { Color::Gray });
        let url_label_style = Style::default().fg(if url_active { Color::Cyan } else { Color::DarkGray });
        let cursor_style = Style::default().fg(Color::Black).bg(Color::Cyan);
        let url_chars: Vec<char> = d.url.chars().collect();
        for row in 0..2usize {
            let start = row * field_w;
            let end   = (start + field_w).min(url_chars.len());
            let label_part = if row == 0 {
                Span::styled(format!("{:indent$}{:<lw$}  ", "", "URL", indent=indent, lw=label_w), url_label_style)
            } else {
                Span::styled(format!("{:width$}", "", width = indent + label_w + 2), Style::default())
            };
            if start > url_chars.len() {
                lines.push(Line::from(label_part));
                continue;
            }
            let mut spans = vec![label_part];
            let row_chars = &url_chars[start..end];
            for (i, ch) in row_chars.iter().enumerate() {
                let abs = start + i;
                let s = ch.to_string();
                if url_active && abs == d.url_cursor {
                    spans.push(Span::styled(s, cursor_style));
                } else {
                    spans.push(Span::styled(s, url_style));
                }
            }
            if url_active && d.url_cursor == url_chars.len() && row == (url_chars.len() / field_w).min(1) {
                spans.push(Span::styled("_", cursor_style));
            }
            lines.push(Line::from(spans));
        }

        // ── Comments ──────────────────────────────────────────────
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  ── Comments ", Style::default().fg(Color::DarkGray)),
            Span::styled("─".repeat(field_w.saturating_sub(2)), Style::default().fg(Color::from_u32(0x303030))),
        ]));
        lines.push(Line::from(""));

        // New comment input (2-line text area)
        let nc_active = d.field == DetailField::NewComment;
        let nc_label_style = Style::default().fg(if nc_active { Color::Cyan } else { Color::DarkGray });
        lines.extend(render_multiline_field(&d.new_comment, d.new_comment_cursor, nc_active, 2, field_w, move |row| {
            if row == 0 {
                Span::styled(format!("{:indent$}{:<lw$}  ", "", "New", indent = indent, lw = label_w), nc_label_style)
            } else {
                Span::styled(format!("{:width$}", "", width = indent + label_w + 2), Style::default())
            }
        }));
        if nc_active {
            lines.push(Line::from(Span::styled(
                format!("{:width$}  Enter to save", "", width = indent + label_w),
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(""));

        // Existing comments (newest first)
        for (ci, comment) in d.comments.iter().enumerate() {
            let is_active = d.field == DetailField::ExistingComment(ci);
            let ts = chrono::DateTime::parse_from_rfc3339(&comment.created_at)
                .map(|dt| dt.format("%d-%m-%Y %H:%M").to_string())
                .unwrap_or_else(|_| comment.created_at.clone());
            lines.push(Line::from(vec![
                Span::styled(format!("  {:indent$}", "", indent = indent), Style::default()),
                Span::styled(ts, Style::default().fg(Color::from_u32(0x505050))),
                if comment.url.is_some() { Span::styled(" link↗", Style::default().fg(Color::Cyan)) }
                else { Span::raw("") },
                if is_active { Span::styled("  [d:delete]", Style::default().fg(Color::from_u32(0x404040))) }
                else { Span::raw("") },
            ]));
            let c_text = if is_active { d.comment_edit_text.as_str() } else { comment.text.as_str() };
            let c_cursor = if is_active { d.comment_edit_cursor } else { 0 };
            lines.extend(render_multiline_field(c_text, c_cursor, is_active, 2, field_w, move |_| {
                Span::styled(format!("{:width$}", "", width = indent + label_w + 2), Style::default())
            }));
            lines.push(Line::from(""));
        }

        // Timestamps
        lines.push(Line::from(""));
        let ts_style = Style::default().fg(Color::from_u32(0x606060));
        let fmt_ts = |ts: &Option<String>| -> String {
            ts.as_deref().map(|s| {
                // Parse RFC3339, format as "DD-MM-YYYY HH:MM"
                chrono::DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.format("%d-%m-%Y %H:%M").to_string())
                    .unwrap_or_else(|_| s.to_string())
            }).unwrap_or_else(|| "—".to_string())
        };
        lines.push(Line::from(vec![
            Span::styled("Created:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(fmt_ts(&d.created_at), ts_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Started:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(fmt_ts(&d.started_at), ts_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Completed:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(fmt_ts(&d.completed_at), ts_style),
        ]));

        frame.render_widget(Clear, area);
        // Manual slice — Paragraph::scroll keeps cursor spans fixed while content moves
        let inner_h = (dialog_h as usize).saturating_sub(2);
        let max_scroll = lines.len().saturating_sub(inner_h);
        let scroll = (d.detail_scroll as usize).min(max_scroll);
        let visible: Vec<Line> = lines.into_iter().skip(scroll).take(inner_h).collect();
        frame.render_widget(Paragraph::new(visible).block(block), area);
    }

    // Due date popup
    if app.due_popup {
        let hint = "Enter:confirm  Esc:cancel  | 3d  fri  eow  W16  16w  DD-MM-YYYY  (empty=clear)";
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
