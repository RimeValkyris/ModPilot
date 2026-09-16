use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::Mutex;

/// How many consecutive crashes (within `WINDOW`) an instance can have
/// before auto-restart gives up on it, rather than looping forever on a
/// server that's crashing because of something a restart can't fix (a bad
/// JVM arg, a corrupted world, a broken mod).
const MAX_CONSECUTIVE_CRASHES: u32 = 3;

/// The window consecutive crashes are counted within. A crash outside this
/// window after the last one starts the count over - an instance that
/// crashed once yesterday and once today isn't "crash-looping".
const WINDOW: Duration = Duration::minutes(10);

struct CrashRecord {
    count: u32,
    first_crash_at: DateTime<Utc>,
}

/// Tracks consecutive crashes per instance, purely in memory - this is
/// about the current session's auto-restart behavior, not a persisted
/// history (that's what `launch_history` in the database is for).
#[derive(Default)]
pub struct CrashTracker {
    records: Mutex<HashMap<String, CrashRecord>>,
}

impl CrashTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a crash and returns `true` if auto-restart should go ahead,
    /// or `false` if this instance has crashed too many times too recently
    /// and auto-restart should give up until the user intervenes.
    pub async fn record_crash_and_check(&self, instance_id: &str) -> bool {
        let mut records = self.records.lock().await;
        let now = Utc::now();

        let record = records.entry(instance_id.to_string()).or_insert(CrashRecord {
            count: 0,
            first_crash_at: now,
        });

        if now - record.first_crash_at > WINDOW {
            // Outside the window - this is a fresh crash streak, not a
            // continuation of the old one.
            record.count = 0;
            record.first_crash_at = now;
        }

        record.count += 1;
        record.count <= MAX_CONSECUTIVE_CRASHES
    }

    /// How many consecutive crashes are currently counted against an
    /// instance. Used to state the number when reporting a crash loop,
    /// rather than the UI having to repeat the threshold constant.
    pub async fn count(&self, instance_id: &str) -> u32 {
        self.records
            .lock()
            .await
            .get(instance_id)
            .map(|r| r.count)
            .unwrap_or(0)
    }

    /// Clears an instance's crash count - called once it successfully
    /// reaches RUNNING, so a server that crashes occasionally but recovers
    /// fine isn't penalized by crashes from long before.
    pub async fn clear(&self, instance_id: &str) {
        self.records.lock().await.remove(instance_id);
    }
}
