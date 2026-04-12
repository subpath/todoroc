use chrono::NaiveDate;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::due_date;

fn topic_alias(name: &str) -> String {
    // Strip leading non-ASCII (emoji) then take first 8 ASCII chars.
    let ascii: String = name
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .collect();
    format!("{:<16.16}", ascii.trim_start())
}

/// Strip the first URL from text for compact display (mirrors todos.rs).
fn strip_url(text: &str) -> String {
    let mut result = text.to_string();
    if let Some(start) = text.find("https://").or_else(|| text.find("http://")) {
        let end = text[start..]
            .find(|c: char| c.is_whitespace())
            .map(|i| start + i)
            .unwrap_or(text.len());
        let before = text[..start].trim_end();
        let after = text[end..].trim_start();
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

pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Overlay: bottom 50% of terminal, min 14 lines
    let overlay_h = (size.height * 50 / 100).max(14).min(size.height);
    let area = Rect {
        x: size.x,
        y: size.y + size.height.saturating_sub(overlay_h),
        width: size.width,
        height: overlay_h,
    };

    let title = if !app.search_results.is_empty() {
        let n = app.search_results.len();
        format!(" Search  ·  {} result{} ", n, if n == 1 { "" } else { "s" })
    } else {
        " Search ".to_string()
    };

    let hint = if !app.search_results.is_empty() {
        " ↑↓:nav  Enter:jump  Esc:close "
    } else {
        " Enter:run  Esc:close "
    };

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(hint, Style::default().fg(Color::DarkGray)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // Query input — cursor always visible since overlay owns all input
    let pending = app.search_debounce.is_some();
    let query_para = Paragraph::new(Line::from(vec![
        Span::styled("Search: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.search_query.clone(), Style::default().fg(Color::White)),
        Span::styled(
            "_",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
        if pending {
            Span::styled("  …", Style::default().fg(Color::DarkGray))
        } else if app.search_query.is_empty() {
            Span::styled("  type to search", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        },
    ]));
    frame.render_widget(query_para, layout[0]);

    if app.search_results.is_empty() {
        if !app.search_query.is_empty() && !pending {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "No results found.",
                    Style::default().fg(Color::DarkGray),
                )),
                layout[1],
            );
        }
        return;
    }

    // Each result is a single line:
    //   score  alias  check  [priority]  [due]  text  [link↗]
    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .map(|(todo, score)| {
            let topic = app.topics.iter().find(|t| t.id == todo.topic_id);
            let topic_name = topic.map(|t| t.name.as_str()).unwrap_or("—");
            let alias = topic_alias(topic_name);

            let (check, check_color) = if todo.done {
                ("[x]", Color::Green)
            } else if todo.in_progress {
                ("[~]", Color::Yellow)
            } else if todo.blocked {
                ("[⊘]", Color::Red)
            } else {
                ("[ ]", Color::White)
            };

            let text_style = if todo.done {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::default()
            };

            let priority_span: Option<Span> = match todo.priority {
                Some(1) => Some(Span::styled("[!!] ", Style::default().fg(Color::Red))),
                Some(2) => Some(Span::styled("[!] ", Style::default().fg(Color::Yellow))),
                Some(3) => Some(Span::styled("[.] ", Style::default().fg(Color::Blue))),
                _ => None,
            };

            let due_badge: Option<(String, Color)> = todo
                .due_date
                .as_deref()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| {
                    let (l, c) = due_date::label(d);
                    (format!("[{}] ", l), c)
                });

            // Dim separator used between score · alias · item info
            let sep = Span::styled(" · ", Style::default().fg(Color::from_u32(0x383838)));

            let priority_len = priority_span
                .as_ref()
                .map(|s| s.content.chars().count())
                .unwrap_or(0);
            let badge_len = due_badge
                .as_ref()
                .map(|(s, _)| s.chars().count())
                .unwrap_or(0);
            let has_url = todo.url.is_some();
            let link_label = " link↗";
            let link_len = if has_url {
                link_label.chars().count()
            } else {
                0
            };

            // overhead: "> "(2) + score(4) + " · "(3) + alias(16) + " · "(3) + check+space(4) + priority + due + url
            let overhead = 2 + 4 + 3 + 16 + 3 + 4 + priority_len + badge_len + link_len;
            let max_text = (area.width as usize).saturating_sub(overhead);

            let display_src = if has_url {
                strip_url(&todo.text)
            } else {
                todo.text.clone()
            };

            let mut spans = vec![
                Span::styled(format!("{:.2}", score), Style::default().fg(Color::Yellow)),
                sep.clone(),
                Span::styled(alias, Style::default().fg(Color::from_u32(0x606060))),
                sep.clone(),
                Span::styled(format!("{} ", check), Style::default().fg(check_color)),
            ];
            if let Some(ps) = priority_span {
                spans.push(ps);
            }
            if let Some((lbl, color)) = due_badge {
                spans.push(Span::styled(lbl, Style::default().fg(color)));
            }
            spans.push(Span::styled(
                super::truncate(&display_src, max_text),
                text_style,
            ));
            if has_url {
                spans.push(Span::styled(link_label, Style::default().fg(Color::Cyan)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.selected_search_result));

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, layout[1], &mut state);
}
