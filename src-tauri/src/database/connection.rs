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
