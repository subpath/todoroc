use chrono::NaiveDate;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{App, BriefingSection};
use crate::due_date;

pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Height: fit content up to 80% of terminal, centred vertically
    let content_rows = count_content_rows(app) as u16;
    let inner_h = content_rows.max(3).min(size.height * 4 / 5);
    let overlay_h = inner_h + 2; // +2 for border
    let overlay_y = size.height.saturating_sub(overlay_h) / 2;

    let area = Rect { x: 0, y: overlay_y, width: size.width, height: overlay_h };

    let date_label = due_date::current_date_label();
    let block = Block::default()
        .title(Span::styled(
            format!(" Daily Focus · {} ", date_label),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " spc:toggle  +/-:snooze  ↵:jump  o:↗  esc:close ",
            Style::default().fg(Color::DarkGray),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    if app.briefing_items.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Nothing to focus on — all clear.",
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
        return;
    }

    let all_lines = build_lines(app, inner.width);
    let selected_line = find_selected_line(app);
    let scroll = compute_scroll(selected_line, inner.height as usize);
    let visible: Vec<Line> = all_lines
        .into_iter()
        .skip(scroll)
        .take(inner.height as usize)
        .collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

fn count_content_rows(app: &App) -> usize {
    let mut count = 0usize;
    let mut prev: Option<&BriefingSection> = None;
    for item in &app.briefing_items {
        if prev.map_or(true, |s| s != &item.section) {
            if prev.is_some() { count += 1; } // blank separator
            count += 1; // section header
            prev = Some(&item.section);
        }
        count += 1;
    }
    count
}

fn find_selected_line(app: &App) -> usize {
    let mut line = 0usize;
    let mut prev: Option<&BriefingSection> = None;
    for (i, item) in app.briefing_items.iter().enumerate() {
        if prev.map_or(true, |s| s != &item.section) {
            if prev.is_some() { line += 1; }
            line += 1;
            prev = Some(&item.section);
        }
        if i == app.selected_briefing { return line; }
        line += 1;
    }
    line
}

fn compute_scroll(selected_line: usize, visible_h: usize) -> usize {
    if selected_line < visible_h { return 0; }
    selected_line.saturating_sub(visible_h / 2)
}

fn strip_url(text: &str) -> String {
    if let Some(start) = text.find("https://").or_else(|| text.find("http://")) {
        let end = text[start..].find(|c: char| c.is_whitespace())
            .map(|i| start + i)
            .unwrap_or(text.len());
        let before = text[..start].trim_end();
        let after  = text[end..].trim_start();
        return if before.is_empty() {
            after.to_string()
        } else if after.is_empty() {
            before.to_string()
        } else {
            format!("{} {}", before, after)
        };
    }
    text.to_string()
}

fn build_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut prev_section: Option<BriefingSection> = None;

    for (i, item) in app.briefing_items.iter().enumerate() {
        // Section header when section changes
        if prev_section.as_ref().map_or(true, |s| s != &item.section) {
            if prev_section.is_some() {
                lines.push(Line::raw(""));
            }
            let (label, color) = match item.section {
                BriefingSection::MustDo      => ("⚡ Must Do",     Color::Red),
                BriefingSection::InFlight    => ("🔄 In Flight",   Color::Yellow),
                BriefingSection::Recommended => ("📋 Recommended", Color::Cyan),
                BriefingSection::Waiting     => ("⊘ Waiting",     Color::DarkGray),
            };
            lines.push(Line::from(Span::styled(
                format!("  {}", label),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            prev_section = Some(item.section.clone());
        }

        let selected = i == app.selected_briefing;

        let (check, check_style): (&'static str, Style) = if item.todo.done {
            ("[x]", Style::default().fg(Color::Green))
        } else if item.todo.in_progress {
            ("[~]", Style::default().fg(Color::Yellow))
        } else if item.todo.blocked {
            ("[⊘]", Style::default().fg(Color::Red))
        } else {
            ("[ ]", Style::default().fg(Color::White))
        };

        let text_style = if item.todo.done {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT)
        } else if selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let priority_span: Option<Span<'static>> = match item.todo.priority {
            Some(1) => Some(Span::styled("[!!] ", Style::default().fg(Color::Red))),
            Some(2) => Some(Span::styled("[!] ",  Style::default().fg(Color::Yellow))),
            Some(3) => Some(Span::styled("[.] ",  Style::default().fg(Color::Blue))),
            _       => None,
        };
        let priority_len = priority_span.as_ref().map(|s| s.content.chars().count()).unwrap_or(0);

        let due_badge: Option<(String, Color)> = item.todo.due_date.as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .map(|d| { let (l, c) = due_date::label(d); (format!("[{}] ", l), c) });
        let badge_len = due_badge.as_ref().map(|(s, _)| s.chars().count()).unwrap_or(0);

        let has_url = item.todo.url.is_some();
        let link_len: usize = if has_url { 2 } else { 0 }; // " ↗"

        // Topic display: strip leading emoji/non-ASCII, cap at 12 chars
        let topic_display: String = {
            let ascii: String = item.topic_name.chars()
                .filter(|c| c.is_ascii() && !c.is_ascii_control())
                .collect();
            let t = ascii.trim_start().to_string();
            super::truncate(&t, 12)
        };
        let topic_len = topic_display.chars().count();

        // selector(2) + check+space(4) + priority + badge + text + link + "  " + topic
        let fixed = 2 + 4 + priority_len + badge_len + link_len + 2 + topic_len;
        let max_text = (width as usize).saturating_sub(fixed);

        let text_src = if has_url { strip_url(&item.todo.text) } else { item.todo.text.clone() };
        let display  = super::truncate(&text_src, max_text);

        let selector_style = if selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut spans: Vec<Span<'static>> = vec![
            Span::styled(if selected { "> " } else { "  " }.to_string(), selector_style),
            Span::styled(format!("{} ", check), check_style),
        ];
        if let Some(ps) = priority_span { spans.push(ps); }
        if let Some((lbl, color)) = due_badge {
            spans.push(Span::styled(lbl, Style::default().fg(color)));
        }
        spans.push(Span::styled(display, text_style));
        if has_url {
            spans.push(Span::styled(" ↗".to_string(), Style::default().fg(Color::Cyan)));
        }
        spans.push(Span::styled(
            format!("  {}", topic_display),
            Style::default().fg(Color::from_u32(0x606060)),
        ));

        lines.push(Line::from(spans));
    }

    lines
}
