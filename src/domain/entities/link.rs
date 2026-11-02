use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Link {
    pub id: i64,
    pub url: String,
    pub label: Option<String>,
    pub entry: DateTime<Utc>,
}

impl Link {
    pub fn display(&self) -> String {
        self.label
            .clone()
            .or_else(|| derive_link_label(&self.url))
            .unwrap_or_else(|| self.url.clone())
    }
}

pub fn is_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.contains("://")
        || s.starts_with("www.")
}

pub fn derive_link_label(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("github.com/"))?;
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 4 {
        let owner = parts[0];
        let repo = parts[1];
        let kind = parts[2];
        let num = parts[3]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or("");
        let tag = match kind {
            "pull" => Some("PR"),
            "issues" => Some("Issue"),
            _ => None,
        };
        if let (Some(tag), false) = (tag, num.is_empty()) {
            return Some(format!("{tag} #{num} · {owner}/{repo}"));
        }
    }
    None
}

pub fn is_issue_link(url: &str) -> bool {
    derive_link_label(url)
        .map(|l| l.starts_with("Issue "))
        .unwrap_or(false)
}

pub fn is_pr_link(url: &str) -> bool {
    derive_link_label(url)
        .map(|l| l.starts_with("PR "))
        .unwrap_or(false)
}

pub fn parse_issue_link(url: &str) -> Option<(String, u64)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("github.com/"))?;
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 4 || parts[2] != "issues" {
        return None;
    }
    let num_str = parts[3]
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("");
    let number: u64 = num_str.parse().ok()?;
    Some((format!("{}/{}", parts[0], parts[1]), number))
}
