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

    let (total_all, total_done) = app
        .topic_counts
        .iter()
        .filter(|(id, _)| **id > 0)
        .fold((0i64, 0i64), |(ta, td), (_, (total, done))| {
            (ta + total, td + done)
        });

    let stats_str = format!(" [{}/{}]", total_done, total_all);
    let stats_color = if total_done == total_all && total_all > 0 {
        Color::Green
    } else {
        Color::Gray
    };

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
                (
                    format!(" [{}/{}]", done, total),
                    if done == total && total > 0 {
                        Color::Green
                    } else {
                        Color::Gray
                    },
                )
            };
            let count = Span::styled(count_str, Style::default().fg(count_color));
            ListItem::new(Line::from(vec![Span::raw(t.name.clone()), count]))
        })
        .collect();

    // Separator between virtual topics (id < 0) and real topics (id > 0)
    let virtual_count = app.topics.iter().filter(|t| t.id < 0).count();
    let has_real_topics = app.topics.iter().any(|t| t.id > 0);
    let has_separator = virtual_count > 0 && has_real_topics;
    if has_separator {
        let sep = ListItem::new(Line::from(Span::styled(
            "  ────────────────────",
            Style::default().fg(Color::from_u32(0x505050)),
        )));
        items.insert(virtual_count, sep);
    }

    // Show input line when inserting.
    // Real topics are offset by 1 in `items` due to the separator.
    let items_offset = |idx: usize| {
        if has_separator && idx >= virtual_count {
            idx + 1
        } else {
            idx
        }
    };

    if focused && app.mode == Mode::Insert {
        let input_text = app
            .input_ta
            .lines()
            .first()
            .map(|s| s.as_str())
            .unwrap_or("");
        let cursor_col = app.input_ta.cursor().1;
        let chars: Vec<char> = input_text.chars().collect();
        let before: String = chars[..cursor_col.min(chars.len())].iter().collect();
        let (cursor_str, after): (String, String) = if cursor_col < chars.len() {
            (
                chars[cursor_col].to_string(),
                chars[cursor_col + 1..].iter().collect(),
            )
        } else {
            ("_".to_string(), String::new())
        };
        let input_line = ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(before),
            Span::styled(
                cursor_str,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
            Span::raw(after),
        ]));
        let display_idx = items_offset(app.selected_topic);
        if app.editing && display_idx < items.len() {
            items[display_idx] = input_line;
        } else {
            items.push(input_line);
        }
    }

    let hint = if focused && app.mode == Mode::Normal {
        " n:new  e:edit  d:del  J/K:reorder  V:toggle views  ↑↓/jk:nav "
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
        state.select(Some(items_offset(app.selected_topic)));
    }

    frame.render_stateful_widget(list, area, &mut state);
}
