use std::collections::HashMap;
use std::path::Path;

use sysinfo::System;
use tauri::State;

use crate::models::{Instance, InstanceRow, PerformanceSample, ResourceUsage};
use crate::server::ping_server;
use crate::AppState;

use super::instance::INSTANCE_COLUMNS;

/// Fills in the metrics that don't come from the process handle: disk,
/// Server List Ping, and the last tick rate the server reported.
///
/// Split out so the single-instance and all-instances commands enrich a
/// sample identically - it would be too easy for the dashboard and an
/// instance page to drift into disagreeing about the same server.
///
/// Every one of these is best-effort. A server that is still booting won't
/// answer a ping, and one whose loader has no tick-rate command never
/// reports TPS at all; both are left as `None` so the UI can say so rather
/// than render a zero that looks like a measurement.
async fn enrich(
    state: &State<'_, AppState>,
    instance: &Instance,
    usage: &mut ResourceUsage,
) {
    if !usage.is_running {
        return;
    }

    let instance_dir = Path::new(&instance.server_directory);
    usage.disk = state.disks.sample(instance_dir).await;

    let port = state.ports.get(&instance.id, instance_dir).await;
    usage.ping = ping_server(port).await;

    if let Some(reading) = state.tps.get(&instance.id).await {
        usage.tps = Some(reading.tps);
        usage.mspt = reading.mspt;
    }
    usage.players_tracked = state.players.list(&instance.id).await.len() as u32;

    // Last, so it judges the fully enriched sample rather than the
    // process-only one.
    usage.health = crate::server::evaluate_health(
        usage.is_running,
        usage.cpu_percent,
        usage.memory_mb,
        Some(instance.max_ram_mb),
        usage.tps,
        usage.mspt,
    );
}

/// Reports the full dashboard picture for one instance, or an all-zero "not
/// running" snapshot if it isn't currently running.
#[tauri::command]
pub async fn get_resource_usage(state: State<'_, AppState>, id: String) -> Result<ResourceUsage, String> {
    let Some((pid, started_at)) = state.processes.running_info(&id).await else {
        return Ok(ResourceUsage::not_running());
    };

    let mut usage = state.resource_monitor.sample(pid, started_at).await;

    let row = sqlx::query_as::<_, InstanceRow>(&format!(
        "SELECT {INSTANCE_COLUMNS} FROM instances WHERE id = ?"
    ))
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| format!("Failed to load instance: {e}"))?;

    if let Some(instance) = row.map(Instance::from) {
        enrich(&state, &instance, &mut usage).await;
    }

    Ok(usage)
}

/// Same as `get_resource_usage`, but for every running instance in one
/// call - the frontend polls this once for the whole app rather than once
/// per visible instance card, which matters once several servers are
/// running simultaneously.
///
/// The instance rows are fetched in a single query rather than one per
/// running server, and the enrichments run concurrently rather than in
/// sequence. Both matter for the same reason: a server that is up but not
/// answering costs a full ping timeout, and serialized that turns N
/// unreachable servers into an N x timeout call while the frontend keeps
/// polling every two seconds.
#[tauri::command]
pub async fn get_all_resource_usage(
    state: State<'_, AppState>,
) -> Result<HashMap<String, ResourceUsage>, String> {
    collect_all_resource_usage(&state, &state.resource_monitor).await
}

/// The body of [`get_all_resource_usage`], parameterized by which
/// [`ResourceMonitor`] does the CPU sampling.
///
/// The parameter exists because `sysinfo` derives CPU percentage from the
/// delta between two refreshes of the *same* `System`. Two callers sharing
/// one monitor therefore corrupt each other: the history recorder running a
/// moment after a dashboard poll would measure CPU over a few milliseconds
/// (reading 0, or a meaningless spike) and reset the baseline the next
/// dashboard poll needed. Giving each its own monitor makes both intervals
/// honest - and the recorder's 60-second delta is the better average for a
/// history series anyway. Everything else here is shared, so the two still
/// agree on memory, ping, TPS and players.
pub(crate) async fn collect_all_resource_usage(
    state: &State<'_, AppState>,
    monitor: &crate::server::ResourceMonitor,
) -> Result<HashMap<String, ResourceUsage>, String> {
    let snapshot = state.processes.running_snapshot().await;
    if snapshot.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, InstanceRow>(&format!("SELECT {INSTANCE_COLUMNS} FROM instances"))
        .fetch_all(&state.db)
        .await
        .map_err(|e| format!("Failed to load instances: {e}"))?;
    let instances: HashMap<String, Instance> = rows
        .into_iter()
        .map(|row| {
            let instance = Instance::from(row);
            (instance.id.clone(), instance)
        })
        .collect();

    // CPU sampling is serialized by the shared `System` behind a mutex, so
    // it happens up front; only the independent I/O below is parallelized.
    let mut sampled = Vec::with_capacity(snapshot.len());
    for (id, pid, started_at) in snapshot {
        sampled.push((id, monitor.sample(pid, started_at).await));
    }

    let enriched = futures::future::join_all(sampled.into_iter().map(|(id, mut usage)| {
        let instances = &instances;
        async move {
            if let Some(instance) = instances.get(&id) {
                enrich(state, instance, &mut usage).await;
            }
            (id, usage)
        }
    }))
    .await;

    Ok(enriched.into_iter().collect())
}

/// Disk usage for the volume that holds the instances directory.
///
/// Separate from `get_resource_usage` because disk is a property of the
/// machine rather than of a process: it is just as meaningful with every
/// server stopped, which is exactly when an operator is deciding whether
/// there is room to install another pack. Gating it on "something is
/// running" would blank the one metric that never stops being true.
#[tauri::command]
pub async fn get_disk_usage(state: State<'_, AppState>) -> Result<Option<crate::models::DiskUsage>, String> {
    Ok(state.disks.sample(&state.paths.instances_dir).await)
}

/// Total physical RAM installed on this machine, in MB - used to show a
/// real "you have N GB available" hint next to an instance's RAM sliders
/// instead of leaving the operator to guess a safe value.
#[tauri::command]
pub fn get_system_memory_mb() -> u64 {
    let mut system = System::new();
    system.refresh_memory();
    system.total_memory() / (1024 * 1024)
}

/// Who is currently online on a running server, from the console-derived
/// roster (see `server::players`). Empty for a stopped instance.
#[tauri::command]
pub async fn list_online_players(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<String>, String> {
    Ok(state.players.list(&id).await)
}

/// How far back `get_performance_history` will look, regardless of what the
/// caller asks for. A guard against a bad argument turning into a query
/// that returns a month of rows into the webview.
const MAX_HISTORY_HOURS: i64 = 24 * 30;

/// Caps the number of rows returned. At one sample per minute, this is a
/// little over a week of continuous uptime - past which a line chart a few
/// hundred pixels wide has more points than pixels anyway.
const MAX_HISTORY_ROWS: i64 = 10_000;

/// Reads an instance's recorded performance history, oldest first.
///
/// Unlike `get_resource_usage`, this works for a stopped instance - that is
/// most of the point. "What was it doing just before it died?" is only
/// answerable after the fact.
#[tauri::command]
pub async fn get_performance_history(
    state: State<'_, AppState>,
    id: String,
    hours: i64,
) -> Result<Vec<PerformanceSample>, String> {
    let hours = hours.clamp(1, MAX_HISTORY_HOURS);
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);

    // Newest-first in SQL so the LIMIT keeps the most *recent* rows when a
    // long window overflows it, then reversed for charting.
    let mut samples = sqlx::query_as::<_, PerformanceSample>(
        "SELECT recorded_at, cpu_percent, memory_mb, tps, mspt, players, ping_ms
         FROM performance_samples
         WHERE instance_id = ? AND recorded_at >= ?
         ORDER BY recorded_at DESC
         LIMIT ?",
    )
    .bind(&id)
    .bind(cutoff)
    .bind(MAX_HISTORY_ROWS)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("Failed to read performance history: {e}"))?;

    samples.reverse();
    Ok(samples)
}
