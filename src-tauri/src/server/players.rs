use std::collections::{HashMap, HashSet};

use tokio::sync::RwLock;

/// Who is currently connected to each running server, derived from the
/// console stream rather than by polling the server with `list`.
///
/// Console-derived because it costs nothing: the log reader is already
/// consuming every line, so this is a string check per line instead of a
/// command round trip every few seconds into a server that may be busy or
/// mid-tick-lag. The tradeoff is that it only reflects what the server
/// printed - it is cleared whenever an instance stops, so a stale roster
/// can never outlive the process it described.
#[derive(Default)]
pub struct PlayerTracker {
    by_instance: RwLock<HashMap<String, HashSet<String>>>,
}

impl PlayerTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one console line in. Returns `true` if the roster changed, so
    /// the caller only emits an event when there is actually news.
    pub async fn observe(&self, instance_id: &str, line: &str) -> bool {
        let Some(event) = parse_player_event(line) else {
            return false;
        };
        let mut map = self.by_instance.write().await;
        let players = map.entry(instance_id.to_string()).or_default();
        match event {
            PlayerEvent::Joined(name) => players.insert(name),
            PlayerEvent::Left(name) => players.remove(&name),
        }
    }

    pub async fn list(&self, instance_id: &str) -> Vec<String> {
        let map = self.by_instance.read().await;
        let mut players: Vec<String> = map.get(instance_id).cloned().unwrap_or_default().into_iter().collect();
        players.sort();
        players
    }

    /// Drops an instance's roster - called when it stops or crashes, so the
    /// UI never shows players still online for a dead server.
    pub async fn clear(&self, instance_id: &str) {
        self.by_instance.write().await.remove(instance_id);
    }
}

#[derive(Debug, PartialEq)]
enum PlayerEvent {
    Joined(String),
    Left(String),
}

/// Recognizes vanilla/Forge/Fabric join and leave lines.
///
/// Deliberately string matching rather than a regex crate call per line:
/// this runs on every single console line of a mod-heavy server's startup
/// (tens of thousands of them), so the common case needs to bail out fast.
///
/// Guards against a chat message impersonating a join notice - a player
/// typing "<Bob> Steve joined the game" produces a line containing the
/// marker, so anything with a chat prefix is rejected outright.
fn parse_player_event(line: &str) -> Option<PlayerEvent> {
    const JOINED: &str = " joined the game";
    const LEFT: &str = " left the game";

    let (marker, is_join) = if line.ends_with(JOINED) {
        (JOINED, true)
    } else if line.ends_with(LEFT) {
        (LEFT, false)
    } else {
        return None;
    };

    // Strip the "[HH:MM:SS] [Server thread/INFO]: " style prefix; the name
    // is whatever sits between the last "]: " and the marker.
    let body = line.strip_suffix(marker)?;
    let body = body.rsplit_once("]: ").map(|(_, rest)| rest).unwrap_or(body).trim();

    // A real join/leave notice is exactly a bare player name.
    if body.is_empty()
        || body.contains(' ')
        || body.contains('<')
        || body.contains('>')
        || body.len() > 32
    {
        return None;
    }

    Some(if is_join {
        PlayerEvent::Joined(body.to_string())
    } else {
        PlayerEvent::Left(body.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vanilla_join_and_leave() {
        assert_eq!(
            parse_player_event("[21:03:11] [Server thread/INFO]: Steve joined the game"),
            Some(PlayerEvent::Joined("Steve".into()))
        );
        assert_eq!(
            parse_player_event("[21:04:52] [Server thread/INFO]: Steve left the game"),
            Some(PlayerEvent::Left("Steve".into()))
        );
    }

    #[test]
    fn ignores_unrelated_and_spoofed_lines() {
        assert_eq!(parse_player_event("[21:03:11] [Server thread/INFO]: Done (12.3s)!"), None);
        // A player typing the marker in chat must not register as a join.
        assert_eq!(
            parse_player_event("[21:03:11] [Server thread/INFO]: <Bob> Steve joined the game"),
            None
        );
        assert_eq!(parse_player_event(""), None);
    }

    #[tokio::test]
    async fn tracks_roster_across_events() {
        let tracker = PlayerTracker::new();
        assert!(tracker.observe("i1", "[x] [Server thread/INFO]: Alice joined the game").await);
        assert!(tracker.observe("i1", "[x] [Server thread/INFO]: Bob joined the game").await);
        assert_eq!(tracker.list("i1").await, vec!["Alice", "Bob"]);

        assert!(tracker.observe("i1", "[x] [Server thread/INFO]: Alice left the game").await);
        assert_eq!(tracker.list("i1").await, vec!["Bob"]);

        tracker.clear("i1").await;
        assert!(tracker.list("i1").await.is_empty());
    }
}
