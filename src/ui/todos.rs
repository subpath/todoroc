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

/// Remove the first URL from text for compact list display.
fn strip_url(text: &str) -> String {
    let mut result = text.to_string();
    if let Some(start) = text.find("https://").or_else(|| text.find("http://")) {
        let end = text[start..].find(|c: char| c.is_whitespace()).map(|i| start + i).unwrap_or(text.len());
        let before = text[..start].trim_end();
        let after  = text[end..].trim_start();
        result = if before.is_empty() {
            after.to_string()
        } else if after.is_empty() {
            before.to_string()
        } else {
            format!("{} {}", before, after)
        };
    }
    result
}

fn display_text(text: &str, max: usize) -> String {
    super::truncate(text, max)
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
            Span::styled(topic_name, dim.add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_style(border_style);

    let mut items: Vec<ListItem> = app
        .todos
        .iter()
        .map(|t| {
            let (check, check_style, text_style) = if t.done {
                ("[x]",
                 Style::default().fg(Color::Green),
                 Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT))
            } else if t.in_progress {
                ("[~]",
                 Style::default().fg(Color::Yellow),
                 Style::default().fg(Color::White))
            } else {
                ("[ ]",
                 Style::default().fg(Color::White),
                 Style::default())
            };
            // Build due date badge first so we can account for its width
            let due_badge: Option<(String, Color)> = t.due_date.as_deref()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| { let (l, c) = due_date::label(d); (format!("[{}] ", l), c) });

            let priority_span = match t.priority {
                Some(1) => Some(Span::styled("[!!] ", Style::default().fg(Color::Red))),
                Some(2) => Some(Span::styled("[!] ", Style::default().fg(Color::Yellow))),
                Some(3) => Some(Span::styled("[.] ", Style::default().fg(Color::Blue))),
                _       => None,
            };
            let priority_len = priority_span.as_ref().map(|s| s.content.chars().count()).unwrap_or(0);

            let has_url = t.url.is_some();
            let display_src = if has_url { strip_url(&t.text) } else { t.text.clone() };
            let link_label = " link↗";
            let badge_len = due_badge.as_ref().map(|(s, _)| s.chars().count()).unwrap_or(0);
            let link_len  = if has_url { link_label.chars().count() } else { 0 };
            let max_text  = (area.width as usize).saturating_sub(12 + badge_len + priority_len + link_len);
            let display   = display_text(&display_src, max_text);

            let mut spans = vec![
                Span::styled(format!("{} ", check), check_style),
            ];
            if let Some(ps) = priority_span {
                spans.push(ps);
            }
            if let Some((lbl, color)) = due_badge {
                spans.push(Span::styled(lbl, Style::default().fg(color)));
            }
            spans.push(Span::styled(display, text_style));
            if has_url {
                spans.push(Span::styled(link_label, Style::default().fg(Color::Cyan)));
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
        hint_owned = format!(" n:new  ↵:detail  e:edit  d:del  @:due  +/-:snooze  p:pri  m:move  spc:toggle  o:↗  {}  /:search ", sort_label);
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
