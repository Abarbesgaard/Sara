use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

pub fn set_estimate(conn: &Connection, task_uuid: &Uuid, mins: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET estimate_mins=?1 WHERE uuid=?2",
        rusqlite::params![mins, task_uuid.to_string()],
    )?;
    Ok(())
}

pub fn activity_counts(
    conn: &Connection,
    days: u32,
    project: Option<&str>,
) -> Result<std::collections::HashMap<chrono::NaiveDate, u32>> {
    let mut map: std::collections::HashMap<chrono::NaiveDate, u32> =
        std::collections::HashMap::new();
    let since = (Utc::now() - chrono::Duration::days(days as i64))
        .format("%Y-%m-%d")
        .to_string();

    let proj_filter = if project.is_some() {
        "AND project=?2"
    } else {
        ""
    };

    {
        let sql = format!(
            "SELECT substr(entry,1,10), COUNT(*) FROM tasks WHERE entry >= ?1 {proj_filter} GROUP BY substr(entry,1,10)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, u32)> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![since, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![since], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };
        for (date_str, count) in rows {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                *map.entry(d).or_insert(0) += count;
            }
        }
    }

    {
        let sql = format!(
            "SELECT substr(end,1,10), COUNT(*) FROM tasks WHERE end IS NOT NULL AND end >= ?1 {proj_filter} GROUP BY substr(end,1,10)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, u32)> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![since, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![since], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };
        for (date_str, count) in rows {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                *map.entry(d).or_insert(0) += count * 2;
            }
        }
    }

    {
        let proj_join = if project.is_some() {
            "JOIN tasks t ON t.uuid = h.task_uuid"
        } else {
            ""
        };
        let proj_where = if project.is_some() {
            "AND t.project=?2"
        } else {
            ""
        };
        let sql = format!(
            "SELECT substr(h.changed_at,1,10), COUNT(*) FROM task_history h {proj_join}
             WHERE h.changed_at >= ?1 {proj_where}
             GROUP BY substr(h.changed_at,1,10)"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(String, u32)> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![since, p], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![since], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };
        for (date_str, count) in rows {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                *map.entry(d).or_insert(0) += count;
            }
        }
    }

    Ok(map)
}

pub fn activity_stats(conn: &Connection, project: Option<&str>) -> Result<(u32, u32, u32, u32)> {
    let proj_filter = if project.is_some() {
        "WHERE project=?1"
    } else {
        ""
    };

    let created: u32 = if let Some(p) = project {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM tasks {proj_filter}"),
            rusqlite::params![p],
            |r| r.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?
    };

    let completed: u32 = if let Some(p) = project {
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status='completed' AND project=?1",
            rusqlite::params![p],
            |r| r.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status='completed'",
            [],
            |r| r.get(0),
        )?
    };

    let mut dates: Vec<chrono::NaiveDate> = {
        let sql = if project.is_some() {
            "SELECT DISTINCT substr(end,1,10) FROM tasks WHERE end IS NOT NULL AND project=?1 ORDER BY end DESC"
        } else {
            "SELECT DISTINCT substr(end,1,10) FROM tasks WHERE end IS NOT NULL ORDER BY end DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<String> = if let Some(p) = project {
            stmt.query_map(rusqlite::params![p], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect()
        };
        rows.iter()
            .filter_map(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .collect()
    };
    dates.sort_unstable();
    dates.dedup();

    let today = Utc::now().date_naive();
    let mut current_streak = 0u32;
    let mut longest_streak = 0u32;
    let mut streak = 0u32;
    let mut prev: Option<chrono::NaiveDate> = None;
    for d in &dates {
        if let Some(p) = prev {
            if (*d - p).num_days() == 1 {
                streak += 1;
            } else {
                streak = 1;
            }
        } else {
            streak = 1;
        }
        longest_streak = longest_streak.max(streak);
        prev = Some(*d);
    }
    if let Some(&last) = dates.last()
        && (today - last).num_days() <= 1
    {
        current_streak = 1;
        let mut d = last;
        for &prev_d in dates.iter().rev().skip(1) {
            if (d - prev_d).num_days() == 1 {
                current_streak += 1;
                d = prev_d;
            } else {
                break;
            }
        }
    }

    Ok((created, completed, current_streak, longest_streak))
}

pub struct ProjectStats {
    pub pending: u32,
    pub active: u32,
    pub completed_total: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub no_pri: u32,
    pub overdue: u32,
    pub due_today: u32,
    pub due_week: u32,
}

pub fn project_stats(conn: &Connection, project: &str) -> Result<ProjectStats> {
    let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let today_str = Utc::now().format("%Y-%m-%d").to_string();
    let week_str = (Utc::now() + chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();

    let pending: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending'",
        [project],
        |r| r.get(0),
    )?;
    let active: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND started_at IS NOT NULL",
        [project], |r| r.get(0),
    )?;
    let completed_total: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='completed'",
        [project],
        |r| r.get(0),
    )?;
    let high: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority='H'",
        [project],
        |r| r.get(0),
    )?;
    let medium: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority='M'",
        [project],
        |r| r.get(0),
    )?;
    let low: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority='L'",
        [project],
        |r| r.get(0),
    )?;
    let no_pri: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND priority IS NULL",
        [project],
        |r| r.get(0),
    )?;
    let overdue: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND due IS NOT NULL AND due < ?2",
        rusqlite::params![project, now_str], |r| r.get(0),
    )?;
    let due_today: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND due IS NOT NULL AND substr(due,1,10)=?2",
        rusqlite::params![project, today_str], |r| r.get(0),
    )?;
    let due_week: u32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project=?1 AND status='pending' AND due IS NOT NULL AND due >= ?2 AND substr(due,1,10) <= ?3",
        rusqlite::params![project, now_str, week_str], |r| r.get(0),
    )?;

    Ok(ProjectStats {
        pending,
        active,
        completed_total,
        high,
        medium,
        low,
        no_pri,
        overdue,
        due_today,
        due_week,
    })
}
