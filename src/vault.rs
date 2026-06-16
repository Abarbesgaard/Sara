use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::Config;
use crate::model::Item;

const PARA_DIRS: &[&str] = &[
    "1 Projects",
    "2 Areas",
    "3 Resources",
    "3 Resources/notes",
    "3 Resources/links",
    "4 Archives",
    "Inbox",
    ".sara",
];

/// Scaffold Sara's private store (idempotent).
pub fn init_store(cfg: &mut Config, path: Option<PathBuf>) -> Result<PathBuf> {
    let store = path.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("Sara")
    });
    fs::create_dir_all(&store).with_context(|| format!("Failed to create {}", store.display()))?;
    for dir in PARA_DIRS {
        fs::create_dir_all(store.join(dir))?;
    }
    crate::memory::init_scaffold(&store)?;
    let profile = store.join(".sara/profile.md");
    if !profile.exists() {
        fs::write(
            &profile,
            "---\ntype: profile\n---\n\n# Sara's profile\n\n_No behavior recorded yet._\n",
        )?;
    }
    let store = store.canonicalize().unwrap_or(store);
    crate::config::set_vault_path(cfg, store.clone())?;
    println!("Sara store ready at {}", store.display());
    Ok(store)
}

pub fn store_root(cfg: &Config) -> Result<PathBuf> {
    let root = crate::config::vault_path(cfg)?;
    if !root.exists() {
        anyhow::bail!(
            "Sara store not initialized. Run `sara init` in the folder where you want the Sara/ directory."
        );
    }
    Ok(root)
}

/// Return the store path, auto-creating Sara/ if missing.
pub fn ensure_store(cfg: &mut Config) -> Result<PathBuf> {
    let root = crate::config::vault_path(cfg)?;
    if root.exists() && cfg.vault_path.is_some() {
        return Ok(root);
    }
    init_store(cfg, None)
}

fn normalize_para_folder(para: Option<&str>) -> &'static str {
    match para {
        Some(p) if p.contains("1 Project") => "1 Projects",
        Some(p) if p.contains("2 Area") => "2 Areas",
        Some(p) if p.contains("Inbox") => "Inbox",
        _ => "3 Resources",
    }
}

pub fn item_relative_path(item: &Item, para_folder: Option<&str>) -> String {
    let base = normalize_para_folder(para_folder);
    let kind_sub = if item.kind == "link" { "links" } else { "notes" };
    let sub = if base == "3 Resources" {
        format!("3 Resources/{kind_sub}")
    } else if base == "Inbox" {
        "Inbox".to_string()
    } else {
        format!("{base}/{kind_sub}")
    };
    format!("{}/{}", sub, item_filename(item))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ItemFrontmatter {
    pub r#type: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in title.chars().take(80) {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

pub fn item_filename(item: &Item) -> String {
    let slug = slugify(&item.title);
    format!("{}-{}.md", slug, &item.uuid.to_string()[..8])
}

pub fn write_item_md(store: &Path, item: &Item, body: &str) -> Result<PathBuf> {
    let rel = item
        .path
        .clone()
        .unwrap_or_else(|| item_relative_path(item, None));
    let full = store.join(rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    let fm = ItemFrontmatter {
        r#type: item.kind.clone(),
        uuid: item.uuid.to_string(),
        title: Some(item.title.clone()),
        url: item.url.clone(),
        tags: item.tags.clone(),
        project: item.project.clone(),
        created: item.created.to_rfc3339(),
        modified: Some(item.modified.to_rfc3339()),
        summary: item.summary.clone(),
    };
    let yaml = serde_yaml::to_string(&fm).context("Failed to serialize frontmatter")?;
    let content = format!("---\n{yaml}---\n\n{body}");
    fs::write(&full, content).with_context(|| format!("Failed to write {}", full.display()))?;
    Ok(full)
}

pub fn parse_item_md(content: &str) -> Result<(ItemFrontmatter, String)> {
    let content = content.strip_prefix("---").context("Missing frontmatter")?;
    let (yaml, body) = content.split_once("---").context("Missing frontmatter end")?;
    let fm: ItemFrontmatter = serde_yaml::from_str(yaml.trim()).context("Invalid frontmatter")?;
    Ok((fm, body.trim().to_string()))
}

pub fn archive_item_md(store: &Path, rel_path: &str) -> Result<()> {
    let src = store.join(rel_path);
    if !src.exists() {
        return Ok(());
    }
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("item.md");
    let dest_dir = store.join("4 Archives");
    fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(name);
    if dest.exists() {
        fs::remove_file(&dest)?;
    }
    fs::rename(&src, &dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_sanitizes_titles() {
        assert_eq!(slugify("Hello World!"), "hello-world");
        assert_eq!(slugify("///"), "untitled");
    }

    #[test]
    fn frontmatter_round_trip() {
        let fm = ItemFrontmatter {
            r#type: "note".into(),
            uuid: Uuid::new_v4().to_string(),
            title: Some("Test".into()),
            url: None,
            tags: vec!["a".into()],
            project: None,
            created: Utc::now().to_rfc3339(),
            modified: None,
            summary: None,
        };
        let yaml = serde_yaml::to_string(&fm).unwrap();
        let parsed: ItemFrontmatter = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.title, fm.title);
    }
}
