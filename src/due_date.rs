use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use ratatui::style::Color;

/// Parse a due date from natural language input.
/// Returns Ok(Some(date)) on success, Ok(None) to clear, Err(msg) on bad input.
pub fn parse(input: &str) -> Result<Option<NaiveDate>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None); // clear due date
    }
    let lower = s.to_lowercase();
    let today = Local::now().date_naive();

    // "today" / "t"
    if lower == "today" || lower == "t" {
        return Ok(Some(today));
    }

    // "tomorrow" / "tom"
    if lower == "tomorrow" || lower == "tom" {
        return Ok(Some(today + Duration::days(1)));
    }

    // "eow" — Friday of the current week (or next Friday if today >= Fri)
    if lower == "eow" {
        let days_to_fri = (4i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
        let days_to_fri = if days_to_fri == 0 { 7 } else { days_to_fri };
        return Ok(Some(today + Duration::days(days_to_fri)));
    }

    // "Nwd" — N working days from now (skipping weekends)
    if let Some(n_str) = lower.strip_suffix("wd")
        && let Ok(n) = n_str.parse::<u32>()
    {
        let mut date = today;
        let mut remaining = n;
        while remaining > 0 {
            date += Duration::days(1);
            if date.weekday().num_days_from_monday() < 5 {
                remaining -= 1;
            }
        }
        return Ok(Some(date));
    }

    // "Nd" — N days from now
    if let Some(n_str) = lower.strip_suffix('d')
        && let Ok(n) = n_str.parse::<i64>()
    {
        return Ok(Some(today + Duration::days(n)));
    }

    // ISO work week — "W16", "w16", or "16w" all mean week 16 of current/next year
    let week_num: Option<u32> = if let Some(n_str) = lower.strip_prefix('w') {
        n_str.parse().ok()
    } else if let Some(n_str) = lower.strip_suffix('w') {
        n_str.parse().ok()
    } else {
        None
    };
    if let Some(week) = week_num
        && (1..=53).contains(&week)
    {
        let year = today.year();
        let date = iso_week_monday(year, week)
            .filter(|d| *d >= today)
            .or_else(|| iso_week_monday(year + 1, week))
            .ok_or_else(|| format!("Invalid work week: {}", week))?;
        return Ok(Some(date));
    }

    // "next <weekday>"
    if let Some(rest) = lower.strip_prefix("next ")
        && let Some(wd) = parse_weekday(rest)
    {
        let days = (wd.num_days_from_monday() as i64
            - today.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        let days = if days == 0 { 7 } else { days } + 7;
        return Ok(Some(today + Duration::days(days)));
    }

    // weekday name — next occurrence (never today)
    if let Some(wd) = parse_weekday(&lower) {
        let days = (wd.num_days_from_monday() as i64
            - today.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        let days = if days == 0 { 7 } else { days };
        return Ok(Some(today + Duration::days(days)));
    }

    // DD-MM-YYYY (with - or . separator)
    let normalized = lower.replace('.', "-");
    if let Ok(d) = NaiveDate::parse_from_str(&normalized, "%d-%m-%Y") {
        return Ok(Some(d));
    }

    // DD-MM without year — use current year
    if let Ok(d) =
        NaiveDate::parse_from_str(&format!("{}-{}", normalized, today.year()), "%d-%m-%Y")
    {
        return Ok(Some(d));
    }

    // YYYY-MM-DD
    if let Ok(d) = NaiveDate::parse_from_str(&lower, "%Y-%m-%d") {
        return Ok(Some(d));
    }

    Err(format!(
        "Can't parse: '{}'. Try: 3d, 3wd, fri, eow, W16, 16w, 20-04-2026",
        s
    ))
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn iso_week_monday(year: i32, week: u32) -> Option<NaiveDate> {
    NaiveDate::from_isoywd_opt(year, week, Weekday::Mon)
}

/// Returns (label, color) for rendering a due date badge.
pub fn label(due: NaiveDate) -> (String, Color) {
    let today = Local::now().date_naive();
    let diff = (due - today).num_days();
    match diff {
        i64::MIN..=-1 => (format!("⚠ {}d ago", -diff), Color::Red),
        0 => ("today".to_string(), Color::Cyan),
        1 => ("tmrw".to_string(), Color::Yellow),
        2..=6 => (due.format("%a").to_string(), Color::Yellow),
        _ => (due.format("%b %-d").to_string(), Color::Gray),
    }
}

/// ISO week number and year label, e.g. "W15".
pub fn current_week_label() -> String {
    let today = Local::now().date_naive();
    format!("W{}", today.iso_week().week())
}

/// e.g. "Wed Apr 8"
pub fn current_date_label() -> String {
    Local::now().date_naive().format("%a %b %-d").to_string()
}

/// Working days remaining in the current quarter (including today if it's a working day).
/// e.g. "Q2 15d"
pub fn quarter_label() -> String {
    let today = Local::now().date_naive();
    let month = today.month();
    let year = today.year();
    let q = (month - 1) / 3 + 1;
    let quarter_end = match month {
        1..=3 => NaiveDate::from_ymd_opt(year, 3, 31).unwrap(),
        4..=6 => NaiveDate::from_ymd_opt(year, 6, 30).unwrap(),
        7..=9 => NaiveDate::from_ymd_opt(year, 9, 30).unwrap(),
        _ => NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
    };
    let mut wd = 0i64;
    let mut d = today;
    while d <= quarter_end {
        if d.weekday().num_days_from_monday() < 5 {
            wd += 1;
        }
        d += Duration::days(1);
    }
    format!("Q{}·{}d", q, wd)
}
