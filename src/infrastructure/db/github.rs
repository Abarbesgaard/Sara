use super::*;
use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct ProjectCommands {
    pub setup_cmd: Option<String>,
    pub test_cmd: Option<String>,
    pub lint_cmd: Option<String>,
    pub run_cmd: Option<String>,
}

pub fn get_project_commands(conn: &Connection, name: &str) -> Result<ProjectCommands> {
    conn.query_row(
        "SELECT setup_cmd, test_cmd, lint_cmd, run_cmd FROM projects WHERE name=?1",
        [name],
        |r| {
            Ok(ProjectCommands {
                setup_cmd: r.get(0)?,
                test_cmd: r.get(1)?,
                lint_cmd: r.get(2)?,
                run_cmd: r.get(3)?,
            })
        },
    )
    .optional()
    .map(|o| o.unwrap_or_default())
    .map_err(Into::into)
}

pub fn set_project_commands(conn: &Connection, name: &str, cmds: &ProjectCommands) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (name, setup_cmd, test_cmd, lint_cmd, run_cmd, last_seen)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(name) DO UPDATE SET
            setup_cmd = COALESCE(?2, setup_cmd),
            test_cmd  = COALESCE(?3, test_cmd),
            lint_cmd  = COALESCE(?4, lint_cmd),
            run_cmd   = COALESCE(?5, run_cmd)",
        params![
            name,
            cmds.setup_cmd,
            cmds.test_cmd,
            cmds.lint_cmd,
            cmds.run_cmd,
            dt_to_str(&Utc::now()),
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct GithubSyncSettings {
    pub repo: Option<String>,
    pub login: Option<String>,
    pub scope: Option<String>,
}

pub fn set_github_sync(conn: &Connection, project: &str, s: &GithubSyncSettings) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (name, github_repo, github_login, github_sync_scope, last_seen)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(name) DO UPDATE SET
           github_repo       = COALESCE(?2, github_repo),
           github_login      = COALESCE(?3, github_login),
           github_sync_scope = COALESCE(?4, github_sync_scope),
           last_seen         = ?5",
        params![project, s.repo, s.login, s.scope, dt_to_str(&Utc::now()),],
    )?;
    Ok(())
}

pub fn get_github_sync(conn: &Connection, project: &str) -> Result<GithubSyncSettings> {
    conn.query_row(
        "SELECT github_repo, github_login, github_sync_scope FROM projects WHERE name=?1",
        [project],
        |r| {
            Ok(GithubSyncSettings {
                repo: r.get(0)?,
                login: r.get(1)?,
                scope: r.get(2)?,
            })
        },
    )
    .optional()
    .map(|o| o.unwrap_or_default())
    .map_err(Into::into)
}

pub fn get_github_provenance(
    conn: &Connection,
    task_uuid: &Uuid,
) -> Result<Option<crate::infrastructure::model::GithubProvenance>> {
    let fields = get_guide_fields(conn, task_uuid)?;
    let Some(raw) = fields.meta_json else {
        return Ok(None);
    };
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let prov = obj.get("github").and_then(|v| {
        serde_json::from_value::<crate::infrastructure::model::GithubProvenance>(v.clone()).ok()
    });
    Ok(prov)
}

pub fn find_github_task_uuid(
    conn: &Connection,
    repo: &str,
    number: i64,
    node_id: Option<&str>,
) -> Result<Option<Uuid>> {
    let mut sql = String::from(
        "SELECT uuid FROM tasks
         WHERE json_extract(meta_json, '$.github.repo') = ?1
           AND (
             json_extract(meta_json, '$.github.number') = ?2",
    );
    if node_id.is_some() {
        sql.push_str(" OR json_extract(meta_json, '$.github.node_id') = ?3");
    }
    sql.push_str(
        ")
         LIMIT 1",
    );

    let row = if let Some(node_id) = node_id {
        conn.query_row(&sql, params![repo, number, node_id], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
    } else {
        conn.query_row(&sql, params![repo, number], |r| r.get::<_, String>(0))
            .optional()?
    };

    Ok(row.and_then(|s| Uuid::parse_str(&s).ok()))
}

pub fn set_github_provenance(
    conn: &Connection,
    task_uuid: &Uuid,
    prov: &crate::infrastructure::model::GithubProvenance,
) -> Result<()> {
    let fields = get_guide_fields(conn, task_uuid)?;
    let mut obj: serde_json::Map<String, serde_json::Value> = fields
        .meta_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    obj.insert(
        "github".to_string(),
        serde_json::to_value(prov).unwrap_or(serde_json::Value::Null),
    );
    let json = serde_json::to_string(&obj)?;
    set_meta_json(conn, task_uuid, &json)
}

pub const NOTE_KIND_GITHUB_COMMENT: &str = "github_comment";

pub fn upsert_github_comment_annotation(
    conn: &Connection,
    task_uuid: &Uuid,
    comment: &crate::infrastructure::model::GithubComment,
) -> Result<bool> {
    let id_str = comment.comment_id.to_string();
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(1) FROM annotations
             WHERE task_uuid=?1 AND target_kind=?2 AND target_id=?3",
            params![task_uuid.to_string(), NOTE_KIND_GITHUB_COMMENT, &id_str],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;
    if exists {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO annotations
           (task_uuid, text, entry, kind, author, target_kind, target_id, status, request_revision)
         VALUES (?1,?2,?3,'comment',?4,?5,?6,'open',0)",
        params![
            task_uuid.to_string(),
            comment.body,
            dt_to_str(&comment.created_at),
            comment.author,
            NOTE_KIND_GITHUB_COMMENT,
            id_str,
        ],
    )?;
    Ok(true)
}

pub fn set_github_comments(
    conn: &Connection,
    task_uuid: &Uuid,
    comments: &[crate::infrastructure::model::GithubComment],
) -> Result<()> {
    let fields = get_guide_fields(conn, task_uuid)?;
    let mut obj: serde_json::Map<String, serde_json::Value> = fields
        .meta_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    obj.insert(
        "github_comments".to_string(),
        serde_json::to_value(comments).unwrap_or(serde_json::Value::Array(vec![])),
    );
    let json = serde_json::to_string(&obj)?;
    set_meta_json(conn, task_uuid, &json)
}

pub fn get_github_comments(
    conn: &Connection,
    task_uuid: &Uuid,
) -> Result<Vec<crate::infrastructure::model::GithubComment>> {
    let fields = get_guide_fields(conn, task_uuid)?;
    let Some(raw) = fields.meta_json else {
        return Ok(Vec::new());
    };
    let obj: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let comments = obj
        .get("github_comments")
        .and_then(|v| {
            serde_json::from_value::<Vec<crate::infrastructure::model::GithubComment>>(v.clone())
                .ok()
        })
        .unwrap_or_default();
    Ok(comments)
}
