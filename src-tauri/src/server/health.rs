//! Reduces a running server's live metrics to one verdict.
//!
//! The dashboard already shows CPU, memory, TPS and MSPT. This answers the
//! question those four numbers exist to answer - "is this server alright?" -
//! so an operator with six instances open does not have to read twenty-four
//! numbers to find the one in trouble.
//!
//! Computed here rather than in the frontend so it rides the resource poll
//! that already happens, and so the thresholds have exactly one definition.
//! It is attached to [`crate::models::ResourceUsage`], not fetched
//! separately.
//!
//! Every verdict carries its reasons. A bare "WARNING" badge with nothing
//! behind it is an invitation to ignore the badge.

use serde::Serialize;

/// Below this the server is missing ticks badly enough that players feel it
/// as rubber-banding and delayed block breaking.
const TPS_CRITICAL: f32 = 15.0;

/// Minecraft's ceiling is 20. A server that cannot hold close to it is
/// already behind, even though the difference is not yet obvious in play.
const TPS_WARNING: f32 = 19.0;

/// A tick's budget is 50 ms. At or past it, the server is by definition
/// unable to keep up.
const MSPT_CRITICAL: f32 = 50.0;

/// Approaching the budget with no headroom for a spike.
const MSPT_WARNING: f32 = 40.0;

/// Fraction of the configured maximum heap that counts as near the limit.
/// Matches `server::alerts`, so the badge and the notification cannot
/// disagree about what "high memory" means.
const MEMORY_HIGH_FRACTION: f64 = 0.90;

/// Sustained CPU this high leaves nothing for a spike, though a single
/// sample this high is normal during chunk generation - which is why this
/// is a warning and never critical on its own.
const CPU_HIGH_PERCENT: f32 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthStatus {
    Critical,
    Warning,
    Healthy,
    /// Not running, so there is nothing to judge.
    Unknown,
}

/// A verdict and the reasons behind it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerHealth {
    pub status: HealthStatus,
    /// Plain statements of what is wrong, empty when nothing is. Shown
    /// under the badge so the verdict is never unexplained.
    pub reasons: Vec<String>,
}

impl ServerHealth {
    pub fn unknown() -> Self {
        Self {
            status: HealthStatus::Unknown,
            reasons: Vec::new(),
        }
    }
}

/// Judges a running server.
///
/// `max_ram_mb` is the instance's configured ceiling; pass `None` when it
/// isn't known, and the memory check is skipped rather than guessed at.
///
/// A metric that is absent is never held against the server. A server whose
/// loader has no tick-rate command is not unhealthy for it - it is just less
/// observable, and reporting that as a warning would mark every Fabric
/// server below 1.20.3 permanently yellow.
pub fn evaluate(
    is_running: bool,
    cpu_percent: f32,
    memory_mb: f64,
    max_ram_mb: Option<i64>,
    tps: Option<f32>,
    mspt: Option<f32>,
) -> ServerHealth {
    if !is_running {
        return ServerHealth::unknown();
    }

    let mut status = HealthStatus::Healthy;
    let mut reasons = Vec::new();

    let mut escalate = |to: HealthStatus, reason: String| {
        if to < status {
            status = to;
        }
        reasons.push(reason);
    };

    if let Some(tps) = tps {
        if tps < TPS_CRITICAL {
            escalate(
                HealthStatus::Critical,
                format!("TPS is {tps:.1} - the server is well behind real time"),
            );
        } else if tps < TPS_WARNING {
            escalate(
                HealthStatus::Warning,
                format!("TPS is {tps:.1}, below the 20.0 ceiling"),
            );
        }
    }

    if let Some(mspt) = mspt {
        if mspt >= MSPT_CRITICAL {
            escalate(
                HealthStatus::Critical,
                format!("{mspt:.0} ms per tick exceeds the 50 ms budget"),
            );
        } else if mspt >= MSPT_WARNING {
            escalate(
                HealthStatus::Warning,
                format!("{mspt:.0} ms per tick leaves little headroom in the 50 ms budget"),
            );
        }
    }

    if let Some(max_ram_mb) = max_ram_mb.filter(|v| *v > 0) {
        let fraction = memory_mb / max_ram_mb as f64;
        if fraction >= MEMORY_HIGH_FRACTION {
            escalate(
                HealthStatus::Warning,
                format!(
                    "Using {:.0} MB of its {max_ram_mb} MB limit",
                    memory_mb
                ),
            );
        }
    }

    if cpu_percent >= CPU_HIGH_PERCENT {
        escalate(
            HealthStatus::Warning,
            format!("CPU at {cpu_percent:.0}%"),
        );
    }

    ServerHealth { status, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_server_is_unknown_not_healthy() {
        let health = evaluate(false, 0.0, 0.0, Some(8192), None, None);
        assert_eq!(health.status, HealthStatus::Unknown);
        assert!(health.reasons.is_empty());
    }

    #[test]
    fn a_comfortable_server_is_healthy_with_no_reasons() {
        let health = evaluate(true, 35.0, 4000.0, Some(8192), Some(20.0), Some(8.0));
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.reasons.is_empty());
    }

    #[test]
    fn low_tps_escalates() {
        assert_eq!(
            evaluate(true, 40.0, 4000.0, Some(8192), Some(18.5), None).status,
            HealthStatus::Warning
        );
        assert_eq!(
            evaluate(true, 40.0, 4000.0, Some(8192), Some(9.0), None).status,
            HealthStatus::Critical
        );
    }

    #[test]
    fn mspt_past_the_tick_budget_is_critical() {
        let health = evaluate(true, 40.0, 4000.0, Some(8192), None, Some(72.0));
        assert_eq!(health.status, HealthStatus::Critical);
        assert!(health.reasons[0].contains("72 ms"));
    }

    #[test]
    fn memory_near_the_configured_ceiling_is_a_warning() {
        let health = evaluate(true, 30.0, 7800.0, Some(8192), Some(20.0), Some(5.0));
        assert_eq!(health.status, HealthStatus::Warning);
        assert!(health.reasons[0].contains("8192 MB limit"));
    }

    /// A server whose loader cannot report a tick rate is not unhealthy for
    /// it - otherwise every Fabric server below 1.20.3 would sit
    /// permanently yellow for a reason its operator cannot act on.
    #[test]
    fn missing_metrics_are_never_held_against_the_server() {
        let health = evaluate(true, 30.0, 4000.0, Some(8192), None, None);
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.reasons.is_empty());

        // Same when the RAM ceiling isn't known.
        let health = evaluate(true, 30.0, 40_000.0, None, Some(20.0), Some(5.0));
        assert_eq!(health.status, HealthStatus::Healthy);
    }

    /// The worst finding wins, but every finding is still reported - the
    /// badge explains itself.
    #[test]
    fn the_worst_finding_wins_and_all_are_listed() {
        let health = evaluate(true, 95.0, 7900.0, Some(8192), Some(8.0), Some(90.0));
        assert_eq!(health.status, HealthStatus::Critical);
        assert_eq!(health.reasons.len(), 4);
    }

    /// A zero or negative RAM ceiling is missing configuration, not a
    /// division to perform.
    #[test]
    fn a_zero_ram_ceiling_skips_the_memory_check() {
        let health = evaluate(true, 30.0, 4000.0, Some(0), Some(20.0), Some(5.0));
        assert_eq!(health.status, HealthStatus::Healthy);
    }
}
