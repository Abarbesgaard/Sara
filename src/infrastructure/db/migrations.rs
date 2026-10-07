use anyhow::{Context, Result};
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

pub(super) fn apply_migrations(conn: &mut Connection) -> Result<()> {
    let migrations = Migrations::new(vec![
        M::up(
            "CREATE TABLE IF NOT EXISTS projects (
                name           TEXT PRIMARY KEY,
                path           TEXT,
                goal           TEXT,
                stack          TEXT,
                conventions    TEXT,
                notes          TEXT,
                initialized_at TEXT,
                last_seen      TEXT
            );
            CREATE TABLE IF NOT EXISTS tasks (
                rowid       INTEGER PRIMARY KEY AUTOINCREMENT,
                uuid        TEXT NOT NULL UNIQUE,
                id          INTEGER,
                description TEXT NOT NULL,
                project     TEXT NOT NULL DEFAULT 'inbox',
                status      TEXT NOT NULL DEFAULT 'pending',
                priority    TEXT,
                due         TEXT,
                entry       TEXT NOT NULL,
                modified    TEXT NOT NULL,
                end         TEXT,
                tags_json   TEXT NOT NULL DEFAULT '[]',
                urgency     REAL NOT NULL DEFAULT 0.0
            );
            CREATE TABLE IF NOT EXISTS dependencies (
                task_uuid       TEXT NOT NULL,
                depends_on_uuid TEXT NOT NULL,
                PRIMARY KEY (task_uuid, depends_on_uuid),
                FOREIGN KEY (task_uuid)       REFERENCES tasks(uuid) ON DELETE CASCADE,
                FOREIGN KEY (depends_on_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS task_files (
                task_uuid TEXT NOT NULL,
                path      TEXT NOT NULL,
                PRIMARY KEY (task_uuid, path),
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            "ALTER TABLE tasks ADD COLUMN started_at TEXT;
             ALTER TABLE tasks ADD COLUMN time_spent INTEGER NOT NULL DEFAULT 0;",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS annotations (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                text      TEXT NOT NULL,
                entry     TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            "ALTER TABLE task_files ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_history (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid  TEXT NOT NULL,
                field      TEXT NOT NULL,
                old_value  TEXT,
                new_value  TEXT,
                changed_at TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_task_history_task
                ON task_history(task_uuid, changed_at);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_links (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                url       TEXT NOT NULL,
                label     TEXT,
                entry     TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS undo_log (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_id    TEXT NOT NULL,
                command     TEXT NOT NULL,
                task_uuid   TEXT NOT NULL,
                before_json TEXT,
                after_json  TEXT,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_undo_log_batch ON undo_log(batch_id);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_branches (
                task_uuid          TEXT PRIMARY KEY,
                branch             TEXT NOT NULL,
                base               TEXT,
                changed_files_json TEXT,
                logged_at          TEXT,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
            );",
        ),
        M::up(
            "ALTER TABLE tasks ADD COLUMN estimate_mins INTEGER;
             CREATE TABLE IF NOT EXISTS task_checklist (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                text      TEXT NOT NULL,
                done      INTEGER NOT NULL DEFAULT 0,
                position  INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
             );",
        ),
        M::up(
            "ALTER TABLE tasks ADD COLUMN recur TEXT;",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS items (
                uuid        TEXT PRIMARY KEY,
                kind        TEXT NOT NULL,
                display_id  INTEGER,
                title       TEXT NOT NULL,
                url         TEXT,
                project     TEXT,
                tags_json   TEXT NOT NULL DEFAULT '[]',
                path        TEXT NOT NULL,
                summary     TEXT,
                body        TEXT NOT NULL DEFAULT '',
                created     TEXT NOT NULL,
                modified    TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'active'
            );
            CREATE TABLE IF NOT EXISTS events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                action      TEXT NOT NULL,
                ref_uuid    TEXT,
                kind        TEXT,
                tags_json   TEXT,
                project     TEXT,
                at          TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS embeddings (
                ref_uuid    TEXT PRIMARY KEY,
                vector_json TEXT NOT NULL
            );",
        ),
        M::up(
            "ALTER TABLE tasks ADD COLUMN assignment TEXT;
             ALTER TABLE tasks ADD COLUMN rationale TEXT;
             ALTER TABLE tasks ADD COLUMN validated_commit TEXT;
             ALTER TABLE tasks ADD COLUMN validated_at TEXT;
             ALTER TABLE tasks ADD COLUMN meta_json TEXT;

             ALTER TABLE task_checklist ADD COLUMN intent TEXT;
             ALTER TABLE task_checklist ADD COLUMN source TEXT NOT NULL DEFAULT 'human';
             ALTER TABLE task_checklist ADD COLUMN kind TEXT NOT NULL DEFAULT 'step';
             ALTER TABLE task_checklist ADD COLUMN verify_cmd TEXT;
             ALTER TABLE task_checklist ADD COLUMN result TEXT;
             ALTER TABLE task_checklist ADD COLUMN done_commit TEXT;
             ALTER TABLE task_checklist ADD COLUMN done_at TEXT;

             ALTER TABLE annotations ADD COLUMN kind TEXT NOT NULL DEFAULT 'comment';
             ALTER TABLE annotations ADD COLUMN author TEXT NOT NULL DEFAULT 'human';
             ALTER TABLE annotations ADD COLUMN target_kind TEXT;
             ALTER TABLE annotations ADD COLUMN target_id TEXT;
             ALTER TABLE annotations ADD COLUMN status TEXT NOT NULL DEFAULT 'open';
             ALTER TABLE annotations ADD COLUMN request_revision INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE annotations ADD COLUMN resolved_by_run INTEGER;

             ALTER TABLE task_files ADD COLUMN reason TEXT;
             ALTER TABLE task_files ADD COLUMN symbol TEXT;
             ALTER TABLE task_files ADD COLUMN line_start INTEGER;
             ALTER TABLE task_files ADD COLUMN line_end INTEGER;

             ALTER TABLE projects ADD COLUMN setup_cmd TEXT;
             ALTER TABLE projects ADD COLUMN test_cmd TEXT;
             ALTER TABLE projects ADD COLUMN lint_cmd TEXT;
             ALTER TABLE projects ADD COLUMN run_cmd TEXT;

             CREATE TABLE IF NOT EXISTS task_ai_runs (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid     TEXT NOT NULL,
                kind          TEXT NOT NULL,
                model         TEXT,
                provider      TEXT,
                prompt        TEXT,
                response_json TEXT,
                created_at    TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_task_ai_runs_task
                ON task_ai_runs(task_uuid, created_at);",
        ),
        M::up(
            "ALTER TABLE projects ADD COLUMN github_repo        TEXT;
             ALTER TABLE projects ADD COLUMN github_login       TEXT;
             ALTER TABLE projects ADD COLUMN github_sync_scope  TEXT;
             CREATE INDEX IF NOT EXISTS idx_projects_github_repo
               ON projects(github_repo) WHERE github_repo IS NOT NULL;",
        ),
        M::up(
            "CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
                ref_kind UNINDEXED, ref_id UNINDEXED, task_uuid UNINDEXED, text
             );

             CREATE TRIGGER IF NOT EXISTS trg_tasks_ai AFTER INSERT ON tasks BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('task', new.uuid, new.uuid,
                    coalesce(new.description,'')||' '||coalesce(new.rationale,'')||' '||coalesce(new.assignment,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_tasks_au AFTER UPDATE ON tasks BEGIN
                DELETE FROM search_index WHERE ref_kind='task' AND ref_id=old.uuid;
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('task', new.uuid, new.uuid,
                    coalesce(new.description,'')||' '||coalesce(new.rationale,'')||' '||coalesce(new.assignment,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_tasks_ad AFTER DELETE ON tasks BEGIN
                DELETE FROM search_index WHERE ref_kind='task' AND ref_id=old.uuid;
             END;

             CREATE TRIGGER IF NOT EXISTS trg_ann_ai AFTER INSERT ON annotations BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('note', new.id, new.task_uuid, coalesce(new.text,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_ann_au AFTER UPDATE ON annotations BEGIN
                DELETE FROM search_index WHERE ref_kind='note' AND ref_id=old.id;
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('note', new.id, new.task_uuid, coalesce(new.text,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_ann_ad AFTER DELETE ON annotations BEGIN
                DELETE FROM search_index WHERE ref_kind='note' AND ref_id=old.id;
             END;

             CREATE TRIGGER IF NOT EXISTS trg_files_ai AFTER INSERT ON task_files BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                VALUES ('anchor', new.rowid, new.task_uuid,
                    coalesce(new.path,'')||' '||coalesce(new.reason,'')||' '||coalesce(new.symbol,''));
             END;
             CREATE TRIGGER IF NOT EXISTS trg_files_ad AFTER DELETE ON task_files BEGIN
                DELETE FROM search_index WHERE ref_kind='anchor' AND ref_id=old.rowid;
             END;

             CREATE VIEW IF NOT EXISTS task_guide AS
             SELECT t.uuid AS uuid, json_object(
                'uuid', t.uuid,
                'id', t.id,
                'description', t.description,
                'project', t.project,
                'status', t.status,
                'priority', t.priority,
                'due', t.due,
                'entry', t.entry,
                'modified', t.modified,
                'tags', json(t.tags_json),
                'urgency', t.urgency,
                'assignment', t.assignment,
                'rationale', t.rationale,
                'validated_commit', t.validated_commit,
                'validated_at', t.validated_at,
                'meta', CASE WHEN t.meta_json IS NOT NULL AND t.meta_json != '' THEN json(t.meta_json) ELSE NULL END,
                'steps', (SELECT json_group_array(json_object(
                        'id', c.id, 'position', c.position, 'text', c.text, 'intent', c.intent,
                        'done', c.done, 'kind', c.kind, 'source', c.source, 'verify_cmd', c.verify_cmd,
                        'result', c.result, 'done_commit', c.done_commit, 'done_at', c.done_at))
                    FROM task_checklist c WHERE c.task_uuid = t.uuid),
                'files', (SELECT json_group_array(json_object(
                        'path', f.path, 'source', f.source, 'reason', f.reason,
                        'symbol', f.symbol, 'line_start', f.line_start, 'line_end', f.line_end))
                    FROM task_files f WHERE f.task_uuid = t.uuid),
                'notes', (SELECT json_group_array(json_object(
                        'id', a.id, 'kind', a.kind, 'author', a.author, 'text', a.text,
                        'target_kind', a.target_kind, 'target_id', a.target_id,
                        'status', a.status, 'request_revision', a.request_revision,
                        'resolved_by_run', a.resolved_by_run, 'entry', a.entry))
                    FROM annotations a WHERE a.task_uuid = t.uuid),
                'links', (SELECT json_group_array(json_object('id', l.id, 'url', l.url, 'label', l.label))
                    FROM task_links l WHERE l.task_uuid = t.uuid),
                'ai_runs', (SELECT json_group_array(json_object(
                        'id', r.id, 'kind', r.kind, 'model', r.model, 'provider', r.provider, 'created_at', r.created_at))
                    FROM task_ai_runs r WHERE r.task_uuid = t.uuid),
                'blocked_by', (SELECT json_group_array(b.id)
                    FROM dependencies d JOIN tasks b ON b.uuid = d.depends_on_uuid
                    WHERE d.task_uuid = t.uuid AND b.status = 'pending')
             ) AS guide_json
             FROM tasks t;",
        ),
        M::up_with_hook(
            "",
            |tx: &rusqlite::Transaction| -> rusqlite_migration::HookResult {
                let existing: std::collections::HashSet<String> = tx
                    .prepare("PRAGMA table_info(projects)")?
                    .query_map([], |r| r.get::<_, String>(1))?
                    .collect::<rusqlite::Result<_>>()?;
                for (col, ddl) in [
                    (
                        "github_repo",
                        "ALTER TABLE projects ADD COLUMN github_repo TEXT",
                    ),
                    (
                        "github_login",
                        "ALTER TABLE projects ADD COLUMN github_login TEXT",
                    ),
                    (
                        "github_sync_scope",
                        "ALTER TABLE projects ADD COLUMN github_sync_scope TEXT",
                    ),
                ] {
                    if !existing.contains(col) {
                        tx.execute_batch(ddl)?;
                    }
                }
                tx.execute_batch(
                    "CREATE INDEX IF NOT EXISTS idx_projects_github_repo \
                     ON projects(github_repo) WHERE github_repo IS NOT NULL;",
                )?;
                Ok(())
            },
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS item_tags (
                item_uuid TEXT NOT NULL,
                tag       TEXT NOT NULL,
                PRIMARY KEY (item_uuid, tag),
                FOREIGN KEY (item_uuid) REFERENCES items(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_item_tags_tag ON item_tags(tag);

             CREATE TABLE IF NOT EXISTS item_projects (
                item_uuid TEXT NOT NULL,
                project   TEXT NOT NULL,
                PRIMARY KEY (item_uuid, project),
                FOREIGN KEY (item_uuid) REFERENCES items(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_item_projects_project ON item_projects(project);",
        ),
        M::up(
            "CREATE TRIGGER IF NOT EXISTS trg_items_ai AFTER INSERT ON items BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                SELECT 'item_' || new.kind, new.uuid, new.uuid,
                    coalesce(new.title,'')||' '||coalesce(new.summary,'')||' '||coalesce(new.body,'')
                WHERE new.status = 'active';
             END;
             CREATE TRIGGER IF NOT EXISTS trg_items_au AFTER UPDATE ON items BEGIN
                DELETE FROM search_index WHERE ref_id = old.uuid AND ref_kind LIKE 'item_%';
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                SELECT 'item_' || new.kind, new.uuid, new.uuid,
                    coalesce(new.title,'')||' '||coalesce(new.summary,'')||' '||coalesce(new.body,'')
                WHERE new.status = 'active';
             END;
             CREATE TRIGGER IF NOT EXISTS trg_items_ad AFTER DELETE ON items BEGIN
                DELETE FROM search_index WHERE ref_id = old.uuid AND ref_kind LIKE 'item_%';
             END;",
        ),
        M::up(
            "ALTER TABLE items ADD COLUMN source_task_uuid TEXT;
             CREATE INDEX IF NOT EXISTS idx_items_source_task
                ON items(source_task_uuid) WHERE source_task_uuid IS NOT NULL;",
        ),
        M::up(
            "ALTER TABLE task_ai_runs ADD COLUMN prompt_tokens     INTEGER;
             ALTER TABLE task_ai_runs ADD COLUMN completion_tokens INTEGER;
             ALTER TABLE task_ai_runs ADD COLUMN total_tokens      INTEGER;",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS item_files (
                item_uuid TEXT NOT NULL,
                file_path TEXT NOT NULL,
                PRIMARY KEY (item_uuid, file_path),
                FOREIGN KEY (item_uuid) REFERENCES items(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_item_files_path ON item_files(file_path);

             CREATE TABLE IF NOT EXISTS item_task_links (
                item_uuid TEXT NOT NULL,
                task_uuid TEXT NOT NULL,
                source    TEXT NOT NULL DEFAULT 'auto',
                PRIMARY KEY (item_uuid, task_uuid),
                FOREIGN KEY (item_uuid) REFERENCES items(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_item_task_links_item ON item_task_links(item_uuid);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS memory_links (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                from_uuid TEXT    NOT NULL,
                to_uuid   TEXT    NOT NULL,
                relation  TEXT    NOT NULL,
                weight    REAL    NOT NULL DEFAULT 1.0,
                created   TEXT    NOT NULL,
                UNIQUE (from_uuid, to_uuid, relation)
             );
             CREATE INDEX IF NOT EXISTS idx_memory_links_from ON memory_links(from_uuid);
             CREATE INDEX IF NOT EXISTS idx_memory_links_to   ON memory_links(to_uuid);",
        ),
        M::up(
            "CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
                ref_kind UNINDEXED, ref_id UNINDEXED, task_uuid UNINDEXED, text
             );
             DROP TRIGGER IF EXISTS trg_items_ai;
             DROP TRIGGER IF EXISTS trg_items_au;
             CREATE TRIGGER trg_items_ai AFTER INSERT ON items BEGIN
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                SELECT 'item_' || new.kind, new.uuid, new.uuid,
                    coalesce(new.title,'')||' '||coalesce(new.summary,'')||' '||coalesce(new.body,'')
                WHERE new.status IN ('active','provisional');
             END;
             CREATE TRIGGER trg_items_au AFTER UPDATE ON items BEGIN
                DELETE FROM search_index WHERE ref_id = old.uuid AND ref_kind LIKE 'item_%';
                INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
                SELECT 'item_' || new.kind, new.uuid, new.uuid,
                    coalesce(new.title,'')||' '||coalesce(new.summary,'')||' '||coalesce(new.body,'')
                WHERE new.status IN ('active','provisional');
             END;
             INSERT INTO search_index(ref_kind, ref_id, task_uuid, text)
             SELECT 'item_' || kind, uuid, uuid,
                 coalesce(title,'')||' '||coalesce(summary,'')||' '||coalesce(body,'')
             FROM items WHERE status = 'provisional';",
        ),
        M::up_with_hook(
            "",
            |tx: &rusqlite::Transaction| -> rusqlite_migration::HookResult {
                let tables: std::collections::HashSet<String> = tx
                    .prepare("SELECT name FROM sqlite_master WHERE type='table'")?
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<_>>()?;
                for (table, ddl) in [
                    (
                        "tasks",
                        "CREATE INDEX IF NOT EXISTS idx_tasks_project_status
                            ON tasks(project, status, urgency DESC)",
                    ),
                    (
                        "tasks",
                        "CREATE INDEX IF NOT EXISTS idx_tasks_status_urgency
                            ON tasks(status, urgency DESC)",
                    ),
                    (
                        "tasks",
                        "CREATE INDEX IF NOT EXISTS idx_tasks_end
                            ON tasks(\"end\" DESC) WHERE \"end\" IS NOT NULL",
                    ),
                    (
                        "tasks",
                        "CREATE INDEX IF NOT EXISTS idx_tasks_id
                            ON tasks(id) WHERE id IS NOT NULL",
                    ),
                    (
                        "annotations",
                        "CREATE INDEX IF NOT EXISTS idx_annotations_task
                            ON annotations(task_uuid, entry)",
                    ),
                    (
                        "task_checklist",
                        "CREATE INDEX IF NOT EXISTS idx_task_checklist_task
                            ON task_checklist(task_uuid, position, id)",
                    ),
                    (
                        "task_links",
                        "CREATE INDEX IF NOT EXISTS idx_task_links_task
                            ON task_links(task_uuid, entry)",
                    ),
                    (
                        "dependencies",
                        "CREATE INDEX IF NOT EXISTS idx_dependencies_depends_on
                            ON dependencies(depends_on_uuid)",
                    ),
                    (
                        "events",
                        "CREATE INDEX IF NOT EXISTS idx_events_action_at
                            ON events(action, at)",
                    ),
                    (
                        "projects",
                        "CREATE INDEX IF NOT EXISTS idx_projects_path
                            ON projects(path, last_seen DESC) WHERE path IS NOT NULL",
                    ),
                ] {
                    if tables.contains(table) {
                        tx.execute_batch(ddl)?;
                    }
                }
                Ok(())
            },
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        ),
        M::up_with_hook(
            "",
            |tx: &rusqlite::Transaction| -> rusqlite_migration::HookResult {
                let has_table: bool = tx
                    .query_row(
                        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='task_branches'",
                        [],
                        |_| Ok(()),
                    )
                    .is_ok();
                if has_table {
                    tx.execute_batch(
                        "ALTER TABLE task_branches DROP COLUMN changed_files_json;
                         ALTER TABLE task_branches DROP COLUMN base;
                         ALTER TABLE task_branches DROP COLUMN logged_at;",
                    )?;
                }
                Ok(())
            },
        ),
        M::up("ALTER TABLE item_files ADD COLUMN fingerprint BLOB;"),
        M::up(
            "CREATE TABLE IF NOT EXISTS memory_uses (
                item_uuid TEXT NOT NULL,
                task_uuid TEXT NOT NULL,
                kind      TEXT NOT NULL CHECK (kind IN ('surfaced', 'recalled', 'cited')),
                at        TEXT NOT NULL,
                PRIMARY KEY (item_uuid, task_uuid, kind),
                FOREIGN KEY (item_uuid) REFERENCES items(uuid) ON DELETE CASCADE,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_memory_uses_task ON memory_uses(task_uuid);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS task_activity (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_uuid TEXT NOT NULL,
                text      TEXT NOT NULL,
                client    TEXT,
                at        TEXT NOT NULL,
                FOREIGN KEY (task_uuid) REFERENCES tasks(uuid) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_task_activity_task ON task_activity(task_uuid, id);",
        ),
    ]);
    migrations
        .to_latest(conn)
        .context("Database migration failed")
}
