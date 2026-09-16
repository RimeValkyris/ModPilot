use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

/// Embeds `src-tauri/migrations` into the binary and runs them against
/// whatever pool is passed in. Safe to call on every startup - already
/// applied migrations are skipped.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens (creating if necessary) the SQLite database at `db_path` and brings
/// its schema up to date.
pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| sqlx::Error::Io(e))?;
    }

    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        // WAL lets readers (e.g. the resource-monitor poll, list_instances)
        // proceed while a write is in progress, instead of the default
        // rollback-journal mode where any writer blocks every other
        // connection in the pool for the duration of its transaction -
        // exactly the pattern this app has a lot of (frequent small writes
        // from status updates, log persistence, and settings, interleaved
        // with frequent reads for polling).
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the real migration set against a fresh database.
    ///
    /// The migrations are embedded at compile time, so a broken one is not a
    /// compile error - it is a startup failure on a user's machine, after
    /// the release is out. This is the only place that catches it.
    #[tokio::test]
    async fn migrations_apply_to_a_fresh_database() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mpp-db-test-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("modpackpilot.db");

        let pool = init_pool(&db_path).await.expect("migrations should apply");

        // Running twice must be a no-op, since every startup does it.
        drop(pool);
        let pool = init_pool(&db_path).await.expect("migrations should be idempotent");

        // The performance history table added in 0009 has to actually work,
        // including its nullable metrics - a null TPS is the normal case for
        // a loader with no tick-rate command.
        sqlx::query(
            "INSERT INTO instances (id, name, loader, server_directory, created_at)
             VALUES ('i1', 'Test', 'forge', '/tmp/i1', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO performance_samples
             (instance_id, recorded_at, cpu_percent, memory_mb, tps, mspt, players, ping_ms)
             VALUES ('i1', '2026-01-01T00:01:00Z', 42.5, 4096.0, NULL, NULL, 3, NULL)",
        )
        .execute(&pool)
        .await
        .expect("performance_samples should accept null metrics");

        let (cpu, tps, players): (f64, Option<f64>, Option<i64>) = sqlx::query_as(
            "SELECT cpu_percent, tps, players FROM performance_samples WHERE instance_id = 'i1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cpu, 42.5);
        assert_eq!(tps, None, "null must round-trip as null, not as zero");
        assert_eq!(players, Some(3));

        // Deleting the instance must take its history with it, or the table
        // grows forever with rows nothing can reach.
        sqlx::query("DELETE FROM instances WHERE id = 'i1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM performance_samples")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0, "samples should cascade with their instance");

        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
