//! The extra dashboard metrics that aren't readable from the OS process.
//!
//! [`crate::server::monitor`] covers CPU, memory and uptime, which all come
//! straight off the process handle. The three metrics here each need their
//! own source:
//!
//! - **Disk** is a property of the volume the instance lives on, not of the
//!   process, so it's sampled from the filesystem.
//! - **Ping and player counts** come from the server's own Server List Ping
//!   port - the same handshake the multiplayer server list performs, which
//!   is the only way to get a latency number that means anything to a player.
//! - **TPS** can only come from the server itself, so it's parsed out of the
//!   console stream that [`crate::server::players`] already reads.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use sysinfo::Disks;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};

use crate::models::{DiskUsage, ServerPing};

// ---------------------------------------------------------------------------
// Disk
// ---------------------------------------------------------------------------

/// How long a volume table stays good for.
///
/// Free space moves slowly next to a 2-second dashboard poll, and
/// enumerating volumes is a blocking syscall that can stall for seconds on a
/// disconnected network drive - so it is done rarely and shared, rather than
/// once per running instance per tick.
const DISK_CACHE_TTL: Duration = Duration::from_secs(10);

/// One volume, flattened out of `sysinfo` so the table can be cached without
/// holding `Disks` (which is neither `Send` nor cheap to keep around).
#[derive(Clone)]
struct Volume {
    mount_point: std::path::PathBuf,
    total_bytes: u64,
    available_bytes: u64,
}

/// Caches the machine's volume table.
///
/// Reports the *volume* an instance sits on, deliberately not the size of the
/// instance folder: walking a modded server directory means stat-ing a world
/// plus a few thousand mod jars, far too expensive at poll cadence - and the
/// number an operator actually needs is "am I about to run out of room",
/// which is a property of the disk.
#[derive(Default)]
pub struct DiskSampler {
    cached: Mutex<Option<(Instant, Vec<Volume>)>>,
}

impl DiskSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The volume usage for whichever mount point contains `dir`.
    ///
    /// The longest matching mount point wins, so a separately mounted
    /// `/home` is picked over `/` rather than whichever the OS listed first.
    pub async fn sample(&self, dir: &Path) -> Option<DiskUsage> {
        let volumes = self.volumes().await;

        let best = volumes
            .iter()
            .filter(|volume| dir.starts_with(&volume.mount_point))
            .max_by_key(|volume| volume.mount_point.as_os_str().len())?;

        if best.total_bytes == 0 {
            return None;
        }

        let used = best.total_bytes.saturating_sub(best.available_bytes);
        Some(DiskUsage {
            used_percent: (used as f64 / best.total_bytes as f64 * 100.0) as f32,
            used_bytes: used,
            total_bytes: best.total_bytes,
            mount_point: best.mount_point.to_string_lossy().to_string(),
        })
    }

    async fn volumes(&self) -> Vec<Volume> {
        {
            let cached = self.cached.lock().await;
            if let Some((at, volumes)) = cached.as_ref() {
                if at.elapsed() < DISK_CACHE_TTL {
                    return volumes.clone();
                }
            }
        }

        // Enumerating volumes blocks, so it must not run on a runtime worker
        // - a stalled network drive would otherwise stall the whole poll.
        let fresh = tokio::task::spawn_blocking(|| {
            Disks::new_with_refreshed_list()
                .iter()
                .map(|disk| Volume {
                    mount_point: disk.mount_point().to_path_buf(),
                    total_bytes: disk.total_space(),
                    available_bytes: disk.available_space(),
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();

        *self.cached.lock().await = Some((Instant::now(), fresh.clone()));
        fresh
    }
}

// ---------------------------------------------------------------------------
// Server List Ping
// ---------------------------------------------------------------------------

/// A ping is only worth the round trip while the server is actually up, and
/// a hung socket must never stall the dashboard poll.
const PING_TIMEOUT: Duration = Duration::from_millis(1500);

/// Caps the status JSON we're willing to read. A well-behaved server sends a
/// few KB; the favicon pushes it higher, but nothing legitimate approaches
/// this, and an unbounded read is a trivial way for a misbehaving process to
/// exhaust memory.
const MAX_STATUS_BYTES: usize = 512 * 1024;

/// Performs a Server List Ping against a running server and returns its
/// latency and player counts.
///
/// This is the handshake Minecraft's own multiplayer list uses: a handshake
/// packet with next-state 1, a status request, then a JSON reply. The
/// latency reported is the wall time of the request/response exchange, which
/// is what the client's connection bars show.
///
/// Returns `None` for anything that isn't a clean answer - the server is
/// still booting, has query disabled, bound to a different interface, or
/// isn't speaking the protocol. A missing ping is displayed as "unavailable"
/// rather than as zero, so guessing here would be worse than declining.
pub async fn ping_server(port: u16) -> Option<ServerPing> {
    let started = Instant::now();
    let result = tokio::time::timeout(PING_TIMEOUT, ping_inner(port)).await;
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;

    let status = result.ok()??;

    // The server answered, so the latency is real regardless of what it chose
    // to disclose. A `players` object is conventional but not guaranteed -
    // count-hiding plugins omit it - and dropping the whole ping over that
    // would report a healthy server as unreachable.
    let players = status.get("players");

    Some(ServerPing {
        latency_ms,
        players_online: players
            .and_then(|p| p.get("online"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        players_max: players
            .and_then(|p| p.get("max"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    })
}

async fn ping_inner(port: u16) -> Option<serde_json::Value> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
    stream.set_nodelay(true).ok();

    // Handshake: packet id 0x00, protocol -1 ("I'm just asking"), host,
    // port, next state 1 (status).
    let mut body = vec![0x00];
    write_varint(&mut body, -1);
    write_string(&mut body, "127.0.0.1");
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, 1);
    write_packet(&mut stream, &body).await?;

    // Status request: packet id 0x00, empty body.
    write_packet(&mut stream, &[0x00]).await?;

    // Reply: length-prefixed packet, id 0x00, then a length-prefixed JSON
    // string.
    let length = read_varint(&mut stream).await?;
    if length <= 0 || length as usize > MAX_STATUS_BYTES {
        return None;
    }
    let mut packet = vec![0u8; length as usize];
    stream.read_exact(&mut packet).await.ok()?;

    let mut cursor = &packet[..];
    if read_varint_slice(&mut cursor)? != 0x00 {
        return None;
    }
    let json_len = read_varint_slice(&mut cursor)?;
    if json_len < 0 || json_len as usize > cursor.len() {
        return None;
    }
    let json = std::str::from_utf8(&cursor[..json_len as usize]).ok()?;

    serde_json::from_str(json).ok()
}

async fn write_packet(stream: &mut TcpStream, body: &[u8]) -> Option<()> {
    let mut framed = Vec::with_capacity(body.len() + 5);
    write_varint(&mut framed, body.len() as i32);
    framed.extend_from_slice(body);
    stream.write_all(&framed).await.ok()?;
    stream.flush().await.ok()
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut remaining = value as u32;
    loop {
        let byte = (remaining & 0x7F) as u8;
        remaining >>= 7;
        if remaining == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    write_varint(out, value.len() as i32);
    out.extend_from_slice(value.as_bytes());
}

async fn read_varint(stream: &mut TcpStream) -> Option<i32> {
    let mut result: i32 = 0;
    for shift in 0..5 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await.ok()?;
        result |= ((byte[0] & 0x7F) as i32) << (shift * 7);
        if byte[0] & 0x80 == 0 {
            return Some(result);
        }
    }
    None
}

fn read_varint_slice(cursor: &mut &[u8]) -> Option<i32> {
    let mut result: i32 = 0;
    for shift in 0..5 {
        let (byte, rest) = cursor.split_first()?;
        *cursor = rest;
        result |= ((byte & 0x7F) as i32) << (shift * 7);
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
    None
}

/// Reads `server-port` out of an instance's `server.properties`.
///
/// Defaults to 25565, which is both Minecraft's default and what the server
/// uses when the key is absent - so a server that has never started once
/// (and therefore has no generated properties file) still gets pinged on the
/// port it will actually come up on.
pub async fn read_server_port(instance_dir: &Path) -> u16 {
    const DEFAULT_PORT: u16 = 25565;

    let path = instance_dir.join("server").join("server.properties");
    let Ok(contents) = tokio::fs::read_to_string(&path).await else {
        return DEFAULT_PORT;
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(("server-port", value)) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
            if let Ok(port) = value.parse::<u16>() {
                if port != 0 {
                    return port;
                }
            }
        }
    }

    DEFAULT_PORT
}

// ---------------------------------------------------------------------------
// TPS
// ---------------------------------------------------------------------------

/// How stale a TPS reading may be before it stops being reported.
///
/// The poller asks every [`TPS_POLL_INTERVAL`], so anything older than a few
/// intervals means the server stopped answering - it's mid-freeze, the
/// command isn't supported, or it's shutting down. Showing the last good
/// number indefinitely would be actively misleading precisely when the
/// server is in trouble, which is exactly when someone is looking at it.
const TPS_MAX_AGE: Duration = Duration::from_secs(35);

/// How often to ask a running server for its tick rate.
///
/// Slow on purpose. Every ask is a command the server has to service, and
/// the answer changes on the order of seconds, so a faster cadence buys
/// nothing and costs a round trip into a server that may already be
/// struggling.
pub const TPS_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// How long after the poller's own request a TPS line is treated as its
/// reply.
///
/// Generous enough for a struggling server to get round to answering, short
/// enough that an operator who runs the command by hand between polls sees
/// their own output.
const TPS_REPLY_WINDOW: Duration = Duration::from_secs(3);

/// One tick-rate report from a server.
///
/// MSPT is the more diagnostic of the two numbers - TPS saturates at 20 and
/// so says nothing about a server that is keeping up comfortably versus one
/// that is one heavy chunk away from falling behind, while milliseconds per
/// tick keeps moving across that whole range. Both are kept because TPS is
/// what operators recognize.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickReading {
    pub tps: f32,
    /// `None` when the server reported a tick rate without a tick time.
    pub mspt: Option<f32>,
}

struct StoredReading {
    reading: TickReading,
    at: Instant,
}

#[derive(Default)]
struct TpsState {
    reading: Option<StoredReading>,
    /// When the poller last sent a tick-rate command to this instance.
    requested_at: Option<Instant>,
}

/// The most recent tick rate each running server reported.
///
/// Fed from the console stream rather than by reading a command's return
/// value, because a server's reply arrives asynchronously on stdout with no
/// correlation to the request that prompted it. The poller writes the
/// command; this catches whatever comes back.
#[derive(Default)]
pub struct TpsTracker {
    by_instance: RwLock<HashMap<String, TpsState>>,
}

impl TpsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that the poller has just asked this instance for its tick
    /// rate, opening the window in which replies are treated as ours.
    pub async fn mark_requested(&self, instance_id: &str) {
        self.by_instance
            .write()
            .await
            .entry(instance_id.to_string())
            .or_default()
            .requested_at = Some(Instant::now());
    }

    /// Feeds one console line in. Returns `true` if the line should be
    /// swallowed rather than shown.
    ///
    /// Only lines that arrive shortly after the poller's own request are
    /// swallowed. An operator who types `neoforge tps` themselves sees the
    /// full reply, which is the whole point of having typed it - and a chat
    /// message that happens to contain "Mean TPS:" is never silently
    /// dropped.
    pub async fn observe(&self, instance_id: &str, line: &str) -> bool {
        if !is_tps_output(line) {
            return false;
        }

        let mut map = self.by_instance.write().await;
        let state = map.entry(instance_id.to_string()).or_default();

        let ours = state
            .requested_at
            .is_some_and(|at| at.elapsed() < TPS_REPLY_WINDOW);

        // The value is worth recording either way - a manually requested
        // reading is just as true as a polled one.
        if let Some(reading) = parse_tick_reading(line) {
            state.reading = Some(StoredReading {
                reading,
                at: Instant::now(),
            });
        }

        ours
    }

    /// The last reported tick rate and tick time, or `None` if the server
    /// has never answered or has stopped answering.
    pub async fn get(&self, instance_id: &str) -> Option<TickReading> {
        let map = self.by_instance.read().await;
        let stored = map.get(instance_id)?.reading.as_ref()?;
        (stored.at.elapsed() < TPS_MAX_AGE).then_some(stored.reading)
    }

    pub async fn clear(&self, instance_id: &str) {
        self.by_instance.write().await.remove(instance_id);
    }
}

/// The console command that asks for a tick rate, for a given loader.
///
/// Forge ships `/forge tps`. NeoForge renamed the whole command tree to
/// `/neoforge` when it split from Forge, so it needs its own spelling -
/// sending `forge tps` there produces an "unknown command" line every poll
/// and never a reading. Vanilla and Fabric have nothing until Minecraft
/// 1.20.3, which added `/tick query` - so a Fabric server older than that
/// simply never answers, and the card reads "unavailable" rather than
/// inventing a number.
pub fn tps_command(loader: &str, minecraft_version: Option<&str>) -> Option<&'static str> {
    match loader.to_ascii_lowercase().as_str() {
        "forge" => Some("forge tps"),
        "neoforge" => Some("neoforge tps"),
        _ => supports_tick_query(minecraft_version?).then_some("tick query"),
    }
}

/// `/tick query` landed in 1.20.3. Anything unparseable is treated as too
/// old, so an odd version string produces no TPS rather than a command the
/// server will reject into the operator's console.
fn supports_tick_query(minecraft_version: &str) -> bool {
    let mut parts = minecraft_version.split('.').map(str::parse::<u32>);
    let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
        return false;
    };
    let patch = parts.next().and_then(Result::ok).unwrap_or(0);

    (major, minor, patch) >= (1, 20, 3)
}

/// Whether a line is part of a tick-rate command's output.
///
/// Broader than [`parse_tick_reading`] on purpose: vanilla's `/tick query` answers
/// with three lines and only the middle one carries a number, so matching
/// solely on the value would leave the other two spilling into the console
/// every poll - exactly what swallowing is meant to prevent.
fn is_tps_output(line: &str) -> bool {
    line.contains("Mean TPS:")
        || line.contains("Mean tick time:")
        || line.contains("Average time per tick:")
        || line.contains("Target tick rate:")
        || line.contains("Percentiles:")
}

/// Pulls a tick rate and tick time out of a console line.
///
/// Handles the two shapes a server can answer in:
///
/// - Forge/NeoForge: `Overall: Mean tick time: 4.2 ms. Mean TPS: 19.8`
///   (and the per-dimension lines that precede it, which carry the same
///   `Mean TPS:` marker - the overall line arrives last and so wins). Both
///   numbers are stated outright.
/// - Vanilla 1.20.3+: `Target tick rate: 20.0 per second. Average time
///   per tick: 4.2ms (Target: 50.0ms)` - no TPS field at all, so the rate
///   is derived from the tick time, capped at the target rate the same way
///   the game does.
fn parse_tick_reading(line: &str) -> Option<TickReading> {
    if let Some(rest) = line.rsplit_once("Mean TPS:").map(|(_, rest)| rest) {
        let tps = parse_leading_f32(rest).filter(|tps| (0.0..=100.0).contains(tps))?;
        // The same line usually carries the tick time before the TPS.
        let mspt = line
            .split_once("Mean tick time:")
            .and_then(|(_, rest)| parse_leading_f32(rest))
            .filter(|ms| *ms >= 0.0);
        return Some(TickReading { tps, mspt });
    }

    // Vanilla reports tick *time*; 20 ticks/s is the ceiling, and a server
    // keeping up reports well under the 50 ms budget.
    if line.contains("Average time per tick:") {
        let rest = line.split_once("Average time per tick:")?.1;
        let ms = parse_leading_f32(rest)?;
        if ms <= 0.0 {
            return None;
        }
        return Some(TickReading {
            tps: (1000.0 / ms).min(20.0),
            mspt: Some(ms),
        });
    }

    None
}

/// Reads the first number out of `text`, ignoring leading spaces and any
/// trailing unit or punctuation.
fn parse_leading_f32(text: &str) -> Option<f32> {
    let trimmed = text.trim_start();
    let end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(trimmed.len());
    trimmed[..end].parse::<f32>().ok()
}

// ---------------------------------------------------------------------------
// Port cache
// ---------------------------------------------------------------------------

/// Remembers each instance's port so the dashboard poll doesn't re-read
/// `server.properties` off disk every couple of seconds for a value that
/// only changes when the operator edits it (and which only takes effect on
/// the next start anyway).
#[derive(Default)]
pub struct PortCache {
    ports: Mutex<HashMap<String, u16>>,
}

impl PortCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, instance_id: &str, instance_dir: &Path) -> u16 {
        if let Some(port) = self.ports.lock().await.get(instance_id) {
            return *port;
        }
        let port = read_server_port(instance_dir).await;
        self.ports.lock().await.insert(instance_id.to_string(), port);
        port
    }

    /// Drops a cached port - called when an instance stops, so an edited
    /// `server-port` is picked up on the next start.
    pub async fn invalidate(&self, instance_id: &str) {
        self.ports.lock().await.remove(instance_id);
    }
}

// ---------------------------------------------------------------------------
// TPS poller
// ---------------------------------------------------------------------------

/// Periodically asks every running server for its tick rate.
///
/// A server only reports TPS when asked, so something has to do the asking.
/// This writes the loader-appropriate command into each running instance's
/// stdin on a slow cadence; the reply comes back through the console stream
/// and is caught by [`TpsTracker::observe`], which also stops it reaching the
/// operator's console view.
///
/// An instance whose loader has no tick-rate command is skipped entirely
/// rather than sent a command the server would reject into its own log.
pub fn spawn_tps_poller(app: tauri::AppHandle) {
    use tauri::Manager;

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TPS_POLL_INTERVAL).await;

            let state = app.state::<crate::AppState>();
            let running: Vec<String> = state
                .processes
                .running_snapshot()
                .await
                .into_iter()
                .map(|(id, _, _)| id)
                .collect();
            if running.is_empty() {
                continue;
            }

            let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
                "SELECT id, loader, minecraft_version FROM instances",
            )
            .fetch_all(&state.db)
            .await;
            let Ok(rows) = rows else { continue };

            for (id, loader, minecraft_version) in rows {
                if !running.contains(&id) {
                    continue;
                }
                let Some(command) = tps_command(&loader, minecraft_version.as_deref()) else {
                    continue;
                };
                // Announce before writing: the reply can land before the
                // write call has even returned, and a reply that arrives
                // before the window opens would reach the operator's console.
                state.tps.mark_requested(&id).await;
                // A write failure here just means the server stopped between
                // the snapshot and now, which the next tick will notice.
                let _ = state.processes.write_line(&id, command).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forge_tps_reports() {
        let overall = parse_tick_reading(
            "[21:03:11] [Server thread/INFO]: Overall: Mean tick time: 4.235 ms. Mean TPS: 19.85",
        )
        .unwrap();
        assert_eq!(overall.tps, 19.85);
        // Forge states the tick time outright, so it must not be discarded.
        assert_eq!(overall.mspt, Some(4.235));

        let per_dim =
            parse_tick_reading("Dim  0 (minecraft:overworld): Mean tick time: 3.1 ms. Mean TPS: 20.00")
                .unwrap();
        assert_eq!(per_dim.tps, 20.0);
        assert_eq!(per_dim.mspt, Some(3.1));
    }

    #[test]
    fn derives_tps_from_vanilla_tick_time() {
        // Vanilla prints these on their own lines, so the test must too -
        // concatenating them would hide the fact that only one carries a
        // value.
        let healthy = parse_tick_reading("Average time per tick: 4.2ms (Target: 50.0ms)").unwrap();
        assert!((healthy.tps - 20.0).abs() < f32::EPSILON);
        assert_eq!(healthy.mspt, Some(4.2));

        // A struggling one: 100 ms per tick is 10 TPS.
        let lagging = parse_tick_reading("Average time per tick: 100.0ms (Target: 50.0ms)").unwrap();
        assert!((lagging.tps - 10.0).abs() < 0.01, "got {}", lagging.tps);
        assert_eq!(lagging.mspt, Some(100.0));
    }

    /// Every line `/tick query` prints must be swallowed, not just the one
    /// carrying the number - otherwise the other two spam the console on
    /// every poll.
    #[test]
    fn recognizes_all_of_a_tick_query_reply() {
        for line in [
            "Target tick rate: 20.0 per second.",
            "Average time per tick: 0.6ms (Target: 50.0ms)",
            "Percentiles: P50: 0.5ms P95: 0.8ms P99: 1.2ms, sample: 100",
            "Overall: Mean tick time: 4.2 ms. Mean TPS: 19.85",
        ] {
            assert!(is_tps_output(line), "should be swallowed: {line}");
        }
        assert!(!is_tps_output("[Server thread/INFO]: <Bob> what's the tps"));
        assert!(!is_tps_output("Done (12.3s)!"));
    }

    #[test]
    fn ignores_unrelated_and_nonsense_lines() {
        assert_eq!(
            parse_tick_reading("[21:03:11] [Server thread/INFO]: Done (12.3s)!"),
            None
        );
        assert_eq!(parse_tick_reading(""), None);
        // Out of range means a misparse, not a real reading.
        assert_eq!(parse_tick_reading("Mean TPS: 4000"), None);
        // Zero tick time would divide by zero.
        assert_eq!(parse_tick_reading("Average time per tick: 0.0ms"), None);
    }

    #[test]
    fn picks_the_right_tps_command_per_loader() {
        assert_eq!(tps_command("forge", Some("1.20.1")), Some("forge tps"));
        // NeoForge renamed the command tree; `forge tps` is an unknown
        // command there.
        assert_eq!(tps_command("neoforge", Some("1.21.1")), Some("neoforge tps"));
        // Fabric only has one on 1.20.3+.
        assert_eq!(tps_command("fabric", Some("1.20.4")), Some("tick query"));
        assert_eq!(tps_command("fabric", Some("1.20.1")), None);
        assert_eq!(tps_command("vanilla", Some("1.21")), Some("tick query"));
        assert_eq!(tps_command("fabric", None), None);
        assert_eq!(tps_command("fabric", Some("not-a-version")), None);
    }

    #[tokio::test]
    async fn swallows_only_the_pollers_own_replies() {
        let tracker = TpsTracker::new();
        const REPORT: &str = "Overall: Mean tick time: 4.2 ms. Mean TPS: 19.5";

        // Unrelated output is never touched.
        assert!(!tracker.observe("i1", "Done (1.2s)!").await);
        assert_eq!(tracker.get("i1").await, None);

        // Typed by the operator, with no poll outstanding: the reading is
        // still recorded, but the line reaches their console.
        assert!(!tracker.observe("i1", REPORT).await);
        let reading = tracker.get("i1").await.unwrap();
        assert_eq!(reading.tps, 19.5);
        assert_eq!(reading.mspt, Some(4.2));

        // Asked for by the poller: swallowed.
        tracker.mark_requested("i1").await;
        assert!(tracker.observe("i1", REPORT).await);

        tracker.clear("i1").await;
        assert_eq!(tracker.get("i1").await, None);
    }
}
