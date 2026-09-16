//! Answering "why is this server unhealthy?" from what is already known.
//!
//! Composition, not new measurement: Java requirements come from
//! [`crate::java::requirement`], mod problems from [`crate::mods`], disk
//! from the [`crate::server::DiskSampler`], crash history from
//! `launch_history`. This module's contribution is turning those into
//! checks with plain-language explanations, and reading the one source
//! nothing else did - the server's own log ([`logs`]).
//!
//! Every check reports `NotApplicable` rather than a pass when it could not
//! actually run, for the same reason the modpack health checks do: a green
//! tick that means "we didn't look" is worse than no tick at all.

pub mod logs;
pub mod report;

pub use report::{build, DiagnosticReport};
