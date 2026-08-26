use std::collections::HashMap;

use chrono::{DateTime, Local, NaiveTime, Utc};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::AppState;

/// How often the scheduler wakes up to look for due work. A minute is
/// fine-grained enough for "daily at 04:00" to fire within a minute of the
/// target, without spinning.
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// A parsed schedule from an instance's `restart_schedule` /
/// `backup_schedule` column. See `migrations/0006_scheduling.sql`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Schedule {
    /// Fire every N hours, measured from when the app last fired it.
    EveryHours(u32),
    /// Fire once per day at this local wall-clock time.
    DailyAt(NaiveTime),
}

impl Schedule {
    pub fn parse(raw: &str) -> Option<Self> {
        let (kind, value) = raw.trim().split_once(':')?;
        match kind {
            "every" => {
                let hours: u32 = value.trim().parse().ok()?;
                (hours > 0).then_some(Schedule::EveryHours(hours))
            }
            // `daily:HH:MM` - the split above only consumed the first colon,
            // so `value` is still the whole "HH:MM".
            "daily" => NaiveTime::parse_from_str(value.trim(), "%H:%M")
                .ok()
                .map(Schedule::DailyAt),
            _ => None,
        }
    }

    /// Whether this schedule is due, given when it last fired.
    ///
    /// `DailyAt` deliberately compares *local* dates: "restart at 4am" means
    /// the operator's own 4am, and it must fire at most once per calendar
    /// day even though the tick runs every minute around that time.
    fn is_due(&self, last_fired: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        match self {
            Schedule::EveryHours(hours) => match last_fired {
                // Never fired this session: start the clock now rather than
                // firing immediately, so launching the app doesn't trigger
                // an instant restart of a server that just came up.
                None => false,
                Some(last) => (now - last).num_minutes() >= (*hours as i64) * 60,
            },
            Schedule::DailyAt(target) => {
                let local_now = now.with_timezone(&Local);
                if local_now.time() < *target {
                    return false;
                }
                match last_fired {
                    None => true,
                    Some(last) => last.with_timezone(&Local).date_naive() < local_now.date_naive(),
                }
            }
        }
    }
}

/// Remembers when each instance's scheduled actions last ran.
///
/// Deliberately in memory rather than persisted: the only cost of losing it
/// on restart is that an "every N hours" timer starts counting from app
/// launch instead of from the last fire, which is harmless. Persisting it
/// would mean a DB write every tick for something this cheap to recompute.
#[derive(Default)]
pub struct ScheduleTracker {
    restarts: Mutex<HashMap<String, DateTime<Utc>>>,
    backups: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl ScheduleTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Starts the background scheduler. One task for the whole app, not one per
/// instance - it re-reads instances every tick, so schedules edited in the
/// UI take effect without restarting anything.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Give startup (including auto-start instances) a moment to settle
        // before the first check.
        tokio::time::sleep(TICK).await;
        loop {
            if let Err(e) = tick(&app).await {
                tracing::warn!("Scheduler tick failed: {e}");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick(app: &AppHandle) -> Result<(), String> {
    let now = Utc::now();

    // Modpack update checks ride the same tick - they are rate-limited
    // internally, so this is cheap on the cycles where nothing is due.
    super::autoupdate::tick(app, now).await;

    let rows: Vec<(String, Option<String>, Option<String>, i64)> = {
        let state = app.state::<AppState>();
        sqlx::query_as("SELECT id, restart_schedule, backup_schedule, backup_keep_last FROM instances")
            .fetch_all(&state.db)
            .await
            .map_err(|e| format!("Failed to read schedules: {e}"))?
    };

    for (id, restart_raw, backup_raw, keep_last) in rows {
        // Both actions only make sense against a live server: restarting a
        // stopped instance would silently *start* it, and backing one up on
        // a schedule it isn't running for just burns disk.
        let running = {
            let state = app.state::<AppState>();
            state.processes.is_running(&id).await
        };
        if !running {
            continue;
        }

        if let Some(schedule) = restart_raw.as_deref().and_then(Schedule::parse) {
            let due = {
                let state = app.state::<AppState>();
                let mut last = state.schedules.restarts.lock().await;
                let due = schedule.is_due(last.get(&id).copied(), now);
                // Either way the clock is (re)started from now: on a fire so
                // the next one is a full interval away, and on the first
                // sighting so an interval has something to measure from.
                last.insert(id.clone(), now);
                due
            };
            if due {
                tracing::info!("Scheduled restart firing for instance {id}");
                let app = app.clone();
                let id = id.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    if let Err(e) =
                        crate::commands::server::restart_instance(app.clone(), state, id.clone()).await
                    {
                        tracing::warn!("Scheduled restart failed for {id}: {e}");
                    }
                });
            }
        }

        if let Some(schedule) = backup_raw.as_deref().and_then(Schedule::parse) {
            let due = {
                let state = app.state::<AppState>();
                let mut last = state.schedules.backups.lock().await;
                let due = schedule.is_due(last.get(&id).copied(), now);
                last.insert(id.clone(), now);
                due
            };
            if due {
                tracing::info!("Scheduled backup firing for instance {id}");
                let app = app.clone();
                let id = id.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    match crate::commands::backup::create_world_backup(state, id.clone()).await {
                        Ok(name) => {
                            tracing::info!("Scheduled backup created for {id}: {name}");
                            if keep_last > 0 {
                                let state = app.state::<AppState>();
                                if let Err(e) =
                                    crate::commands::backup::prune_backups(&state, &id, keep_last).await
                                {
                                    tracing::warn!("Backup rotation failed for {id}: {e}");
                                }
                            }
                        }
                        Err(e) => tracing::warn!("Scheduled backup failed for {id}: {e}"),
                    }
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_schedule_forms() {
        assert_eq!(Schedule::parse("every:6"), Some(Schedule::EveryHours(6)));
        assert_eq!(
            Schedule::parse("daily:04:00"),
            Some(Schedule::DailyAt(NaiveTime::from_hms_opt(4, 0, 0).unwrap()))
        );
        assert_eq!(Schedule::parse("every:0"), None);
        assert_eq!(Schedule::parse("nonsense"), None);
        assert_eq!(Schedule::parse("daily:99:99"), None);
    }

    #[test]
    fn every_hours_waits_for_the_interval() {
        let s = Schedule::EveryHours(6);
        let now = Utc::now();
        // Never fired -> starts the clock, does not fire instantly.
        assert!(!s.is_due(None, now));
        assert!(!s.is_due(Some(now - chrono::Duration::hours(5)), now));
        assert!(s.is_due(Some(now - chrono::Duration::hours(6)), now));
    }
}
