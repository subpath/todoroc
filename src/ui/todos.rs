use chrono::NaiveDate;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::app::{App, Focus, Mode, TodoSort};
use crate::due_date;

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut)
    }
}

fn display_text(text: &str, max: usize) -> String {
    let stripped = text.strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    if stripped.len() < text.len() {
        truncate(stripped, max.min(50))
    } else {
        truncate(text, max)
    }
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Todos;
    let topic_name = app
        .topics
        .get(app.selected_topic)
        .map(|t| t.name.as_str())
        .unwrap_or("—");

    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let dim = Style::default().fg(Color::from_u32(0x808080)); // medium gray
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" Items", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("  ·  ", dim),
            Span::styled(due_date::current_date_label(), dim),
            Span::styled("  ", dim),
            Span::styled(due_date::current_week_label(), dim),
            Span::styled("  ·  ", dim),
            Span::styled(topic_name, dim.add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_style(border_style);

    let mut items: Vec<ListItem> = app
        .todos
        .iter()
        .map(|t| {
            let check = if t.done { "[x]" } else { "[ ]" };
            let check_style = if t.done {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            let text_style = if t.done {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::default()
            };
            // Build due date badge first so we can account for its width
            let due_badge: Option<(String, Color)> = t.due_date.as_deref()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| { let (l, c) = due_date::label(d); (format!("[{}] ", l), c) });

            let badge_len = due_badge.as_ref().map(|(s, _)| s.chars().count()).unwrap_or(0);
            // reserve: 2 border + 2 highlight symbol + 4 check + badge + 2 url indicator
            let max_text = (area.width as usize).saturating_sub(12 + badge_len);
            let display = display_text(&t.text, max_text);

            let mut spans = vec![
                Span::styled(format!("{} ", check), check_style),
            ];
            if let Some((lbl, color)) = due_badge {
                spans.push(Span::styled(lbl, Style::default().fg(color)));
            }
            spans.push(Span::styled(display, text_style));
            if t.url.is_some() {
                spans.push(Span::styled(" ↗", Style::default().fg(Color::Cyan)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    if focused && app.mode == Mode::Insert {
        let chars: Vec<char> = app.input.chars().collect();
        let before: String = chars[..app.cursor_pos.min(chars.len())].iter().collect();
        let (cursor_str, after): (String, String) = if app.cursor_pos < chars.len() {
            (chars[app.cursor_pos].to_string(), chars[app.cursor_pos + 1..].iter().collect())
        } else {
            ("_".to_string(), String::new())
        };
        let input_line = ListItem::new(Line::from(vec![
            Span::styled("[ ] ", Style::default().fg(Color::DarkGray)),
            Span::raw(before),
            Span::styled(cursor_str, Style::default().fg(Color::Cyan).add_modifier(Modifier::SLOW_BLINK)),
            Span::raw(after),
        ]));
        if app.editing && app.selected_todo < items.len() {
            items[app.selected_todo] = input_line;
        } else {
            items.push(input_line);
        }
    }

    let sort_label = match app.todo_sort {
        TodoSort::Bucketed => "s:sort[bucketed]",
        TodoSort::Flat     => "s:sort[flat]",
    };
    let hint_owned;
    let hint = if focused && app.mode == Mode::Normal {
        hint_owned = format!(" n:new  e:edit  d:del  @:due  spc:toggle  o:open↗  {}  ↑↓/jk:nav ", sort_label);
        hint_owned.as_str()
    } else {
        ""
    };

    let items_len = items.len();
    let list = List::new(items)
        .block(block.title_bottom(hint.to_string()))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if focused && app.mode == Mode::Insert && !app.editing {
        // Scroll to the new input line at the bottom
        state.select(Some(items_len.saturating_sub(1)));
    } else if !app.todos.is_empty() {
        state.select(Some(app.selected_todo));
    }

    frame.render_stateful_widget(list, area, &mut state);
}
