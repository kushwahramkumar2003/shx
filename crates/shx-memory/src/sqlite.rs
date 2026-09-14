//! SQLite [`MemoryStore`]: bundled rusqlite, WAL, `user_version` migrations.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Row, params};
use shx_core::{
    Interaction, PrunePolicy, PruneReport, RiskLevel, Scope, Snippet, Verdict, VocabEntry,
    VocabSource,
};

use crate::paths::{ensure_parent, restrict_file};
use crate::store::{MemoryError, MemoryStore, Result};

const DAY_MS: i64 = 86_400_000;
const INIT_SQL: &str = include_str!("migrations/001_init.sql");

/// File-backed store. One connection per process (WAL).
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) `path`, migrate to v1, set Unix file perms.
    pub fn open(path: &Path) -> Result<Self> {
        let created = !path.exists();
        if created {
            ensure_parent(path)?;
        }
        let conn = Connection::open(path)?;
        configure(&conn)?;
        migrate(&conn)?;
        if created {
            restrict_file(path)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Ephemeral DB for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))
    }

    /// Insert a shell-history line (redacted). Used by `import-history` (T-605).
    pub fn record_shell(
        &self,
        ts: i64,
        cwd: Option<&str>,
        cmd: &str,
        exit_code: Option<i32>,
        source: &str,
    ) -> Result<i64> {
        let cmd = crate::record::prepare_shell_cmd(cmd);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO shell_history (ts, cwd, cmd, exit_code, source) VALUES (?1,?2,?3,?4,?5)",
            params![ts, cwd, cmd, exit_code, source],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(Duration::from_millis(5_000))?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let v: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v < 1 {
        conn.execute_batch(INIT_SQL)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn json_vec(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn parse_vec(s: Option<String>) -> Vec<String> {
    s.and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn risk_str(r: RiskLevel) -> &'static str {
    match r {
        RiskLevel::Safe => "safe",
        RiskLevel::Review => "review",
        RiskLevel::Danger => "danger",
    }
}

fn parse_risk(s: String) -> RiskLevel {
    match s.as_str() {
        "review" => RiskLevel::Review,
        "danger" => RiskLevel::Danger,
        _ => RiskLevel::Safe,
    }
}

fn source_str(s: VocabSource) -> &'static str {
    match s {
        VocabSource::Taught => "taught",
        VocabSource::Learned => "learned",
        VocabSource::Imported => "imported",
    }
}

fn parse_source(s: String) -> VocabSource {
    match s.as_str() {
        "taught" => VocabSource::Taught,
        "imported" => VocabSource::Imported,
        _ => VocabSource::Learned,
    }
}

fn map_interaction(row: &Row<'_>) -> rusqlite::Result<Interaction> {
    Ok(Interaction {
        id: Some(row.get(0)?),
        ts: row.get(1)?,
        session_id: row.get(2)?,
        project_id: row.get(3)?,
        cwd: row.get(4)?,
        os: row.get(5)?,
        shell: row.get(6)?,
        input_nl: row.get(7)?,
        output_cmd: row.get(8)?,
        explanation: row.get(9)?,
        backend: row.get(10)?,
        model: row.get(11)?,
        confidence: row.get::<_, Option<f64>>(12)?.map(|f| f as f32),
        latency_ms: row.get::<_, i64>(13)? as u64,
        risk_level: parse_risk(row.get(14)?),
        risk_notes: parse_vec(row.get(15)?),
        from_cache: row.get::<_, i64>(16)? != 0,
        accepted: match row.get::<_, Option<i64>>(17)? {
            None => None,
            Some(0) => Some(false),
            Some(_) => Some(true),
        },
        executed: match row.get::<_, Option<i64>>(18)? {
            None => None,
            Some(0) => Some(false),
            Some(_) => Some(true),
        },
        tags: parse_vec(row.get(19)?),
    })
}

fn select_sql(_scope: &Scope, extra: &str, limit_pos: u32) -> String {
    format!(
        "SELECT id, ts, session_id, project_id, cwd, os, shell, input_nl, output_cmd,
                explanation, backend, model, confidence, latency_ms, risk_level,
                risk_notes, from_cache, accepted, executed, tags
         FROM interactions {extra} ORDER BY ts DESC, id DESC LIMIT ?{limit_pos}"
    )
}

impl MemoryStore for SqliteStore {
    fn record_interaction(&self, i: &Interaction) -> Result<i64> {
        let i = crate::record::prepare_interaction(i);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO interactions (
                ts, session_id, project_id, cwd, os, shell, input_nl, output_cmd,
                explanation, backend, model, confidence, latency_ms, risk_level,
                risk_notes, from_cache, accepted, executed, tags
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                i.ts,
                i.session_id,
                i.project_id,
                i.cwd,
                i.os,
                i.shell,
                i.input_nl,
                i.output_cmd,
                i.explanation,
                i.backend,
                i.model,
                i.confidence.map(f64::from),
                i.latency_ms as i64,
                risk_str(i.risk_level),
                json_vec(&i.risk_notes),
                i.from_cache as i64,
                i.accepted.map(i64::from),
                i.executed.map(i64::from),
                json_vec(&i.tags),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn recent(&self, limit: usize, scope: Scope) -> Result<Vec<Interaction>> {
        let conn = self.lock()?;
        let lim = limit as i64;
        let rows = match &scope {
            Scope::Project { id: Some(pid) } => {
                let sql = select_sql(&scope, "WHERE project_id = ?1", 2);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![pid, lim], map_interaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            Scope::Project { id: None } => {
                let sql = select_sql(&scope, "WHERE project_id IS NOT NULL", 1);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![lim], map_interaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            Scope::Shell => {
                let sql = select_sql(&scope, "WHERE 0=1", 1);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![lim], map_interaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            Scope::Tool => {
                let sql = select_sql(&scope, "WHERE 1=1", 1);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![lim], map_interaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        Ok(rows)
    }

    fn search(&self, query: &str, limit: usize, scope: Scope) -> Result<Vec<Interaction>> {
        let conn = self.lock()?;
        let like = format!("%{}%", query.replace('%', "\\%"));
        let lim = limit as i64;
        let like_pred =
            "AND (lower(input_nl) LIKE lower(?{q}) OR lower(output_cmd) LIKE lower(?{q}))";
        let rows = match &scope {
            Scope::Project { id: Some(pid) } => {
                let extra = format!("WHERE project_id = ?1 {}", like_pred.replace("{q}", "2"));
                let sql = select_sql(&scope, &extra, 3);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![pid, like, lim], map_interaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            other => {
                let where_ = match other {
                    Scope::Tool => "WHERE 1=1",
                    Scope::Shell => "WHERE 0=1",
                    Scope::Project { id: None } => "WHERE project_id IS NOT NULL",
                    Scope::Project { id: Some(_) } => unreachable!(),
                };
                let extra = format!("{where_} {}", like_pred.replace("{q}", "1"));
                let sql = select_sql(other, &extra, 2);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![like, lim], map_interaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        Ok(rows)
    }

    fn vocabulary(&self, terms: &[String]) -> Result<Vec<VocabEntry>> {
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.lock()?;
        let mut out = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT term, expansion, weight, source, last_used_ts, use_count
             FROM vocabulary WHERE term = ?1",
        )?;
        for t in terms {
            let rows = stmt.query_map(params![t.to_ascii_lowercase()], |row| {
                Ok(VocabEntry {
                    term: row.get(0)?,
                    expansion: row.get(1)?,
                    weight: row.get(2)?,
                    source: parse_source(row.get(3)?),
                    last_used_ts: row.get(4)?,
                    use_count: row.get::<_, i64>(5)? as u64,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    fn upsert_vocabulary(&self, e: &VocabEntry) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO vocabulary (term, expansion, weight, source, last_used_ts, use_count)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(term, expansion) DO UPDATE SET
               weight=excluded.weight,
               source=excluded.source,
               last_used_ts=excluded.last_used_ts,
               use_count=excluded.use_count",
            params![
                e.term.to_ascii_lowercase(),
                e.expansion,
                e.weight,
                source_str(e.source),
                e.last_used_ts,
                e.use_count as i64,
            ],
        )?;
        Ok(())
    }

    fn snippets(&self) -> Result<Vec<Snippet>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, command, description, created_ts, use_count FROM snippets",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Snippet {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    command: row.get(2)?,
                    description: row.get(3)?,
                    created_ts: row.get(4)?,
                    use_count: row.get::<_, i64>(5)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn feedback(&self, interaction_id: i64, v: Verdict, note: Option<&str>) -> Result<()> {
        let conn = self.lock()?;
        let exists: Option<i64> = conn
            .query_row(
                "SELECT id FROM interactions WHERE id = ?1",
                params![interaction_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(MemoryError::Message(format!(
                "no interaction {interaction_id}"
            )));
        }
        let accepted = match v {
            Verdict::Good => 1i64,
            Verdict::Bad => 0,
        };
        conn.execute(
            "UPDATE interactions SET accepted = ?1 WHERE id = ?2",
            params![accepted, interaction_id],
        )?;
        conn.execute(
            "INSERT INTO feedback (interaction_id, verdict, note, ts) VALUES (?1,?2,?3,?4)",
            params![
                interaction_id,
                match v {
                    Verdict::Good => "good",
                    Verdict::Bad => "bad",
                },
                note,
                now_ms(),
            ],
        )?;
        Ok(())
    }

    fn prune(&self, policy: &PrunePolicy) -> Result<PruneReport> {
        let conn = self.lock()?;
        let now = now_ms();
        let cutoff = now.saturating_sub(i64::from(policy.retention_days) * DAY_MS);
        let deleted = conn.execute("DELETE FROM interactions WHERE ts < ?1", params![cutoff])?;
        let mut stripped = 0usize;
        if !policy.keep_danger {
            let danger_cut = now.saturating_sub(30 * DAY_MS);
            stripped = conn.execute(
                "UPDATE interactions SET output_cmd = ''
                 WHERE risk_level = 'danger' AND ts < ?1 AND output_cmd != ''",
                params![danger_cut],
            )?;
        }
        let decayed = conn.execute(
            "UPDATE vocabulary SET weight = weight * 0.98
             WHERE last_used_ts < ?1",
            params![now.saturating_sub(30 * DAY_MS)],
        )?;
        Ok(PruneReport {
            interactions_deleted: deleted as u64,
            vocab_decayed: decayed as u64,
            danger_commands_stripped: stripped as u64,
        })
    }
}
