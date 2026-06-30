use chrono::{DateTime, Utc};

/// Preset due values cycled with ←/→ in the review form's Due field.
pub const DUE_PRESETS: &[&str] = &[
    "",
    "today",
    "tomorrow",
    "+2d",
    "+3d",
    "+1w",
    "+2w",
    "next monday",
    "next friday",
];

/// Parse a human-friendly due string into a UTC datetime.
/// Handles ISO dates, `+Nd`/`+Nw` shorthand, "today", and natural language via interim.
pub fn parse_due(s: &str, dialect_str: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let dialect = match dialect_str {
        "us" => interim::Dialect::Us,
        _ => interim::Dialect::Uk,
    };

    // ISO date
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date.and_hms_opt(23, 59, 59)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    }

    // "today"
    if s.eq_ignore_ascii_case("today") {
        return Some(Utc::now());
    }

    // Relative: +3d, +2w
    if let Some(rest) = s.strip_prefix('+') {
        if let Some(days_str) = rest.strip_suffix('d')
            && let Ok(days) = days_str.trim().parse::<i64>()
        {
            return Some(Utc::now() + chrono::Duration::days(days));
        }
        if let Some(weeks_str) = rest.strip_suffix('w')
            && let Ok(weeks) = weeks_str.trim().parse::<i64>()
        {
            return Some(Utc::now() + chrono::Duration::weeks(weeks));
        }
    }

    // Natural language via interim
    if let Ok(dt) = interim::parse_date_string(s, chrono::Local::now(), dialect) {
        return Some(dt.with_timezone(&Utc));
    }

    None
}

/// True if the string parses to a valid due date (or is empty).
pub fn is_valid_due(s: &str) -> bool {
    let s = s.trim();
    s.is_empty() || parse_due(s, "uk").is_some()
}
