use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
};

use crate::app::{App, Focus, Mode};
use super::focused_block;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Topics;
    let block = focused_block("Topics", focused);

    let mut items: Vec<ListItem> = app
        .topics
        .iter()
        .map(|t| ListItem::new(Line::from(t.name.clone())))
        .collect();

    // Show input line when inserting
    if focused && app.mode == Mode::Insert {
        let chars: Vec<char> = app.input.chars().collect();
        let before: String = chars[..app.cursor_pos.min(chars.len())].iter().collect();
        let (cursor_str, after): (String, String) = if app.cursor_pos < chars.len() {
            (chars[app.cursor_pos].to_string(), chars[app.cursor_pos + 1..].iter().collect())
        } else {
            ("_".to_string(), String::new())
        };
        let input_line = ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(before),
            Span::styled(cursor_str, Style::default().fg(Color::Cyan).add_modifier(Modifier::SLOW_BLINK)),
            Span::raw(after),
        ]));
        if app.editing && app.selected_topic < items.len() {
            items[app.selected_topic] = input_line;
        } else {
            items.push(input_line);
        }
    }

    let hint = if focused && app.mode == Mode::Normal {
        " 1/2/3:focus  n:new  e:edit  d:del  ↑↓/jk:nav "
    } else {
        ""
    };

    let list = List::new(items)
        .block(block.title_bottom(hint))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    if !app.topics.is_empty() {
        state.select(Some(app.selected_topic));
    }

    frame.render_stateful_widget(list, area, &mut state);
}
