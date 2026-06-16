use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::llm::ItemEnrichmentResponse;
use crate::model::Item;
use crate::vault;

pub fn is_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://") || s.starts_with("https://")
}

pub fn fetch_link_title(url: &str) -> Option<String> {
    let resp = reqwest::blocking::get(url).ok()?;
    let html = resp.text().ok()?;
    extract_title(&html).or_else(|| extract_og_title(&html))
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title>")? + 7;
    let end = lower[start..].find("</title>")? + start;
    let title = html.get(start..end)?.trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn extract_og_title(html: &str) -> Option<String> {
    for line in html.lines() {
        let l = line.to_lowercase();
        if l.contains("og:title") {
            if let Some(start) = line.find("content=\"") {
                let rest = &line[start + 9..];
                if let Some(end) = rest.find('"') {
                    let t = rest[..end].trim().to_string();
                    if !t.is_empty() {
                        return Some(t);
                    }
                }
            }
        }
    }
    None
}

fn apply_item_enrichment(item: &mut Item, enrichment: &ItemEnrichmentResponse) -> Option<String> {
    for tag in &enrichment.tags {
        if !item.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            item.tags.push(tag.clone());
        }
    }
    if let Some(ref summary) = enrichment.summary {
        item.summary = Some(summary.clone());
    }
    if let Some(ref title) = enrichment.title {
        let t = title.trim();
        if !t.is_empty() && t.len() <= 120 {
            item.title = t.to_string();
        }
    }
    enrichment.para_folder.clone()
}

pub fn capture_note(
    conn: &Connection,
    cfg: &mut Config,
    text: &str,
    use_llm: bool,
) -> Result<Item> {
    let store = vault::ensure_store(cfg)?;
    let title: String = text
        .lines()
        .next()
        .unwrap_or("Untitled")
        .chars()
        .take(80)
        .collect();
    let mut item = Item::new_note(title, text.to_string());

    let para_folder = if use_llm {
        let (enrichment, err) =
            crate::enrich::enrich_item(cfg, "note", &item.title, &item.body, None);
        if let Some(msg) = err {
            eprintln!("Note enrichment skipped: {msg}");
        }
        apply_item_enrichment(&mut item, &enrichment)
    } else {
        None
    };

    item.path = Some(vault::item_relative_path(&item, para_folder.as_deref()));
    vault::write_item_md(&store, &item, &item.body)?;
    db::insert_item(conn, &mut item)?;
    db::record_event(
        conn,
        "capture",
        Some(&item.uuid),
        Some(&item.kind),
        &item.tags,
        item.project.as_deref(),
    )?;
    let embed_text = format!(
        "{} {} {}",
        item.title,
        item.body,
        item.summary.as_deref().unwrap_or("")
    );
    crate::embed::embed_and_store(conn, cfg, &item.uuid, &embed_text)?;
    crate::memory::observe_capture(
        cfg,
        &item.kind,
        &item.handle(),
        &item.title,
        item.summary.as_deref(),
    );
    if use_llm {
        crate::memory::extract_observations(
            cfg,
            &item.body,
            &format!("Captured as {}: {}", item.handle(), item.title),
            "capture-note",
        );
    }
    println!("Captured note {}: {}", item.handle(), item.title);
    Ok(item)
}

pub fn capture_link(
    conn: &Connection,
    cfg: &mut Config,
    url: &str,
    note: Option<&str>,
    use_llm: bool,
) -> Result<Item> {
    let store = vault::ensure_store(cfg)?;
    let title = fetch_link_title(url).unwrap_or_else(|| url.to_string());
    let body = note.unwrap_or("").to_string();
    let mut item = Item::new_link(url.to_string(), title.clone(), body.clone());

    let para_folder = if use_llm {
        let (enrichment, err) = crate::enrich::enrich_item(
            cfg,
            "link",
            &item.title,
            &body,
            Some(url),
        );
        if let Some(msg) = err {
            eprintln!("Link enrichment skipped: {msg}");
        }
        apply_item_enrichment(&mut item, &enrichment)
    } else {
        None
    };

    item.path = Some(vault::item_relative_path(&item, para_folder.as_deref()));
    vault::write_item_md(&store, &item, &body)?;
    db::insert_item(conn, &mut item)?;
    db::record_event(
        conn,
        "capture",
        Some(&item.uuid),
        Some(&item.kind),
        &item.tags,
        item.project.as_deref(),
    )?;
    let embed_text = format!(
        "{} {} {} {}",
        item.title,
        url,
        body,
        item.summary.as_deref().unwrap_or("")
    );
    crate::embed::embed_and_store(conn, cfg, &item.uuid, &embed_text)?;
    crate::memory::observe_capture(
        cfg,
        &item.kind,
        &item.handle(),
        &item.title,
        item.summary.as_deref(),
    );
    if use_llm {
        let user_text = format!("{url} {body}");
        crate::memory::extract_observations(
            cfg,
            &user_text,
            &format!("Captured as {}: {}", item.handle(), item.title),
            "capture-link",
        );
    }
    println!("Captured link {}: {}", item.handle(), item.title);
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_urls() {
        assert!(is_url("https://example.com"));
        assert!(!is_url("hello world"));
    }

    #[test]
    fn extracts_title_from_html() {
        let html = "<html><head><title>Hello</title></head></html>";
        assert_eq!(extract_title(html), Some("Hello".into()));
    }

    #[test]
    fn apply_enrichment_merges_tags_and_summary() {
        let mut item = Item::new_note("Old".into(), "body".into());
        let enrichment = ItemEnrichmentResponse {
            summary: Some("Short summary".into()),
            tags: vec!["rust".into()],
            para_folder: Some("3 Resources".into()),
            title: Some("Better title".into()),
        };
        let para = apply_item_enrichment(&mut item, &enrichment);
        assert_eq!(item.title, "Better title");
        assert_eq!(item.summary.as_deref(), Some("Short summary"));
        assert!(item.tags.contains(&"rust".to_string()));
        assert_eq!(para.as_deref(), Some("3 Resources"));
    }
}
