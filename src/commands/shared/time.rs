use chrono::{DateTime, Utc};

pub fn rel_time(dt: DateTime<Utc>) -> String {
    let secs = (Utc::now() - dt).num_seconds().max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match secs {
        s if s < MIN => "just now".to_string(),
        s if s < HOUR => format!("{}m ago", s / MIN),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 30 * DAY => format!("{}d ago", s / DAY),
        s if s < 365 * DAY => format!("{}mo ago", s / (30 * DAY)),
        s => format!("{}y ago", s / (365 * DAY)),
    }
}

pub fn month_abbr(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
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
