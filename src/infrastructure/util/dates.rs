use chrono::{DateTime, Utc};

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

pub fn parse_due(s: &str, dialect_str: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let dialect = match dialect_str {
        "us" => interim::Dialect::Us,
        _ => interim::Dialect::Uk,
    };

    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date.and_hms_opt(23, 59, 59)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    }

    if s.eq_ignore_ascii_case("today") {
        return Some(Utc::now());
    }

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

    if let Ok(dt) = interim::parse_date_string(s, chrono::Local::now(), dialect) {
        return Some(dt.with_timezone(&Utc));
    }

    None
}

pub fn parse_duration_mins(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s_lower = s.to_lowercase();
    let rest = s_lower.as_str();
    if let Some(h_pos) = rest.find('h')
        && let Ok(h) = rest[..h_pos].trim().parse::<i64>()
    {
        let mut total = h * 60;
        let after_h = rest[h_pos + 1..].trim().trim_end_matches('m').trim();
        if !after_h.is_empty()
            && let Ok(m) = after_h.parse::<i64>()
        {
            total += m;
        }
        return Some(total);
    }
    let m_part = rest.trim_end_matches('m').trim();
    m_part.parse::<i64>().ok()
}

pub fn is_valid_due(s: &str) -> bool {
    let s = s.trim();
    s.is_empty() || parse_due(s, "uk").is_some()
}
