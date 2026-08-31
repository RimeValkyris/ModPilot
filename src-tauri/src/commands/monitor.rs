use std::collections::HashMap;
use std::path::Path;

use sysinfo::System;
use tauri::State;

use crate::models::{Instance, InstanceRow, ResourceUsage};
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

    usage.tps = state.tps.get(&instance.id).await;
    usage.players_tracked = state.players.list(&instance.id).await.len() as u32;
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
        sampled.push((id, state.resource_monitor.sample(pid, started_at).await));
    }

    let enriched = futures::future::join_all(sampled.into_iter().map(|(id, mut usage)| {
        let state = &state;
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
