use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};

use crate::app::{App, Focus};
use super::focused_block;


pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Search;
    let block = focused_block("Semantic Search", focused);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // Query input line
    let query_line = Line::from(vec![
        Span::styled("Search: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.search_query.clone(),
            Style::default().fg(Color::White),
        ),
        if focused && app.mode == crate::app::Mode::Insert {
            Span::styled("_", Style::default().fg(Color::Cyan).add_modifier(Modifier::SLOW_BLINK))
        } else {
            Span::raw("")
        },
    ]);
    let hint = if app.search_query.is_empty() {
        Span::styled(
            "  (n to type, Enter to search)",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::raw("")
    };

    let query_para = Paragraph::new(Line::from(vec![
        query_line.spans[0].clone(),
        query_line.spans[1].clone(),
        query_line.spans[2].clone(),
        hint,
    ]));
    frame.render_widget(query_para, layout[0]);

    // Results
    if app.search_results.is_empty() && !app.search_query.is_empty() {
        let no_results = Paragraph::new(Span::styled(
            "No results found.",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(no_results, layout[1]);
    } else {
        let items: Vec<ListItem> = app
            .search_results
            .iter()
            .map(|(todo, score)| {
                let topic_icon: Option<char> = app.topics.iter()
                    .find(|t| t.id == todo.topic_id)
                    .and_then(|t| t.name.chars().next())
                    .filter(|c| !c.is_ascii());

                let (check, check_color) = if todo.done {
                    ("[x]", Color::Green)
                } else if todo.in_progress {
                    ("[~]", Color::Yellow)
                } else {
                    ("[ ]", Color::White)
                };
                let icon_w = if topic_icon.is_some() { 3 } else { 0 };
                let overhead = 7 + 4 + icon_w + 4;
                let max_text = (area.width as usize).saturating_sub(overhead);
                let display = super::truncate(&todo.text, max_text);

                let mut spans = vec![
                    Span::styled(format!("{:.2} ", score), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{} ", check), Style::default().fg(check_color)),
                ];
                if let Some(icon) = topic_icon {
                    spans.push(Span::styled(format!("{} ", icon), Style::default().fg(Color::Magenta)));
                }
                spans.push(Span::raw(display));
                if todo.url.is_some() {
                    spans.push(Span::styled(" ↗", Style::default().fg(Color::Cyan)));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let mut state = ratatui::widgets::ListState::default();
        if !app.search_results.is_empty() {
            state.select(Some(app.selected_search_result));
        }
        let list = List::new(items)
            .highlight_style(
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, layout[1], &mut state);
    }
}
