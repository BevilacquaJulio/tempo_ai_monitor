use crate::providers::{antigravity::Reading, Observation};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_secs(3))?;
    if db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? == 0 {
        db.execute_batch(concat!("BEGIN;\n", include_str!("../../migrations/001_history.sql"), "\nCOMMIT;"))?;
    }
    if db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? == 1 {
        db.execute_batch(concat!("BEGIN IMMEDIATE;\n", include_str!("../../migrations/002_antigravity_reading.sql"), "\nCOMMIT;"))?;
    }
    if db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? == 2 {
        db.execute_batch(concat!("BEGIN IMMEDIATE;\n", include_str!("../../migrations/003_claude_reading.sql"), "\nCOMMIT;"))?;
    }
    Ok(db)
}

pub fn save_antigravity(db: &Connection, path: &str, reading: &Reading) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO antigravity_reading VALUES (1, ?1, ?2, ?3)
        ON CONFLICT(id) DO UPDATE SET source_path=excluded.source_path, observed_at=excluded.observed_at, payload=excluded.payload
        WHERE excluded.source_path != antigravity_reading.source_path OR excluded.observed_at >= antigravity_reading.observed_at",
        params![path, reading.at, serde_json::to_string(reading).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?],
    )?;
    Ok(())
}

pub fn read_antigravity(db: &Connection) -> rusqlite::Result<Option<(String, Reading)>> {
    db.query_row("SELECT source_path, payload FROM antigravity_reading WHERE id = 1", [], |row| {
        let payload: String = row.get(1)?;
        let reading = serde_json::from_str(&payload).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
        })?;
        Ok((row.get(0)?, reading))
    })
    .optional()
}

pub fn save_claude(db: &Connection, reading: &Observation) -> rusqlite::Result<()> {
    let payload = serde_json::to_string(reading).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    db.execute(
        "INSERT INTO claude_reading VALUES (1, ?1, ?2)
        ON CONFLICT(id) DO UPDATE SET observed_at=excluded.observed_at, payload=excluded.payload
        WHERE excluded.observed_at >= claude_reading.observed_at",
        params![reading.at, payload],
    )?;
    Ok(())
}

pub fn read_claude(db: &Connection) -> rusqlite::Result<Option<Observation>> {
    db.query_row("SELECT payload FROM claude_reading WHERE id = 1", [], |row| {
        let payload: String = row.get(0)?;
        serde_json::from_str(&payload).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
        })
    })
    .optional()
}

pub fn clear_claude(db: &Connection) -> rusqlite::Result<()> {
    db.execute("DELETE FROM claude_reading", [])?;
    Ok(())
}

pub fn write(db: &mut Connection, provider: &str, rows: &[(i64, Value)]) -> rusqlite::Result<()> {
    let tx = db.transaction()?;
    {
        let mut statement = tx.prepare_cached(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id, external_id) DO UPDATE
             SET last_activity=excluded.last_activity, payload=excluded.payload
             WHERE excluded.last_activity >= sessions.last_activity
                OR (sessions.payload LIKE '%\"totalTokens\":null%' AND excluded.payload NOT LIKE '%\"totalTokens\":null%')",
        )?;
        for (last, row) in rows {
            if let Some(id) = row["id"].as_str() {
                statement.execute(params![provider, id, last, row.to_string()])?;
            }
        }
    }
    tx.commit()
}

pub fn read(db: &Connection, cutoff: i64) -> rusqlite::Result<Vec<Value>> {
    let mut query = db.prepare("SELECT payload FROM sessions WHERE last_activity >= ?1 ORDER BY last_activity DESC")?;
    let rows = query.query_map([cutoff], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let row = row?;
        serde_json::from_str(&row).map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err)))
    })
    .collect()
}

pub fn reserve_alert(db: &Connection, provider: &str, window: &str, reset: i64, threshold: u8) -> rusqlite::Result<bool> {
    Ok(db.execute("INSERT OR IGNORE INTO alert_deliveries VALUES (?1, ?2, ?3, ?4)", params![provider, window, reset, threshold])? == 1)
}

pub fn clear_cutoff(db: &Connection) -> rusqlite::Result<i64> {
    db.query_row("SELECT COALESCE(MAX(cleared_at), 0) FROM history_reset", [], |row| row.get(0))
}

pub fn set_clear_cutoff(db: &Connection, cutoff: i64) -> rusqlite::Result<()> {
    db.execute("INSERT OR REPLACE INTO history_reset VALUES (1, ?1)", [cutoff])?;
    Ok(())
}

pub fn clear_history(db: &mut Connection, cutoff: i64) -> rusqlite::Result<()> {
    db.pragma_update(None, "secure_delete", true)?;
    let tx = db.transaction()?;
    tx.execute("DELETE FROM sessions", [])?;
    set_clear_cutoff(&tx, cutoff)?;
    tx.commit()
}

pub fn prune(db: &Connection, session_cutoff: i64, now: i64) -> rusqlite::Result<()> {
    db.pragma_update(None, "secure_delete", true)?;
    db.execute("DELETE FROM sessions WHERE last_activity < ?1", [session_cutoff])?;
    db.execute("DELETE FROM alert_deliveries WHERE resets_at < ?1", [now.saturating_sub(86400)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runs_v1_to_v3_migrations_on_clean_and_upgraded_databases() {
        assert!(rusqlite::version_number() >= 3_054_000, "bundled SQLite must be >= 3.54.0");
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let fresh_path = dir.path().join("fresh.sqlite");
        let fresh = open(&fresh_path).unwrap();
        let version: i64 = fresh.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 3);

        let v1_path = dir.path().join("v1.sqlite");
        let raw = Connection::open(&v1_path).unwrap();
        raw.execute_batch(include_str!("../../migrations/001_history.sql")).unwrap();
        drop(raw);
        let upgraded = open(&v1_path).unwrap();
        let upgraded_version: i64 = upgraded.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(upgraded_version, 3);

        let newer = Observation {
            plan: Some("Pro".into()),
            quotas: vec![json!({"id":"claude-five_hour-","type":"five_hour","usedPercent":42.0,"status":"available"})],
            at: 2000,
            daily_tokens: Vec::new(),
        };
        let older = Observation { at: 1000, ..newer.clone() };
        save_claude(&upgraded, &newer).unwrap();
        save_claude(&upgraded, &older).unwrap();
        let loaded = read_claude(&upgraded).unwrap().unwrap();
        assert_eq!(loaded.at, 2000);
        assert_eq!(loaded.plan.as_deref(), Some("Pro"));
        clear_claude(&upgraded).unwrap();
        assert!(read_claude(&upgraded).unwrap().is_none());
    }

    #[test]
    fn deduplicates_sessions_and_preserves_latest_observation() {
        let mut db = open(Path::new(":memory:")).unwrap();
        write(&mut db, "codex", &[(20, json!({"id":"a","totalTokens":40}))]).unwrap();
        write(&mut db, "codex", &[(10, json!({"id":"a","totalTokens":20}))]).unwrap();
        assert_eq!(read(&db, 0).unwrap(), vec![json!({"id":"a","totalTokens":40})]);
        clear_history(&mut db, 25).unwrap();
        assert!(read(&db, 0).unwrap().is_empty());
        assert_eq!(clear_cutoff(&db).unwrap(), 25);
    }

    #[test]
    fn alerts_are_independent_by_cycle_window_provider_and_threshold() {
        let db = open(Path::new(":memory:")).unwrap();
        assert!(reserve_alert(&db, "codex", "weekly", 20, 90).unwrap());
        assert!(!reserve_alert(&db, "codex", "weekly", 20, 90).unwrap());
        assert!(reserve_alert(&db, "codex", "five_hour", 20, 90).unwrap());
        assert!(reserve_alert(&db, "claude", "weekly", 20, 90).unwrap());
        assert!(reserve_alert(&db, "codex", "weekly", 30, 90).unwrap());
        assert!(reserve_alert(&db, "codex", "weekly", 20, 95).unwrap());
    }
}
