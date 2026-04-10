use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::app::{App, Focus, Mode};
use crate::due_date;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {

    let focused = app.focus == Focus::Topics;

    let (total_all, total_done) = app.topic_counts.iter()
        .filter(|(id, _)| **id > 0)
        .fold((0i64, 0i64), |(ta, td), (_, (total, done))| (ta + total, td + done));

    let stats_str = format!(" [{}/{}]", total_done, total_all);
    let stats_color = if total_done == total_all && total_all > 0 { Color::Green } else { Color::Gray };

    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let dim = Style::default().fg(Color::from_u32(0x808080));

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" Topics", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("  ·  ", dim),
            Span::styled(due_date::current_date_label(), dim),
            Span::styled("  ", dim),
            Span::styled(due_date::current_week_label(), dim),
            Span::styled(stats_str, Style::default().fg(stats_color)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_style(border_style);

    let mut items: Vec<ListItem> = app
        .topics
        .iter()
        .map(|t| {
            let (total, done) = app.topic_counts.get(&t.id).copied().unwrap_or((0, 0));
            let (count_str, count_color) = if t.id < 0 {
                (format!(" [{}]", total), Color::Gray)
            } else {
                (format!(" [{}/{}]", done, total), if done == total && total > 0 { Color::Green } else { Color::Gray })
            };
            let count = Span::styled(count_str, Style::default().fg(count_color));
            ListItem::new(Line::from(vec![Span::raw(t.name.clone()), count]))
        })
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
