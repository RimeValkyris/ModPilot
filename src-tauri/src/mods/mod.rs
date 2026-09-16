//! Reading and checking the contents of an instance's `mods/` folder.
//!
//! Distinct from `commands::mods`, which is the Tauri surface for listing,
//! toggling and deleting mod files. This module is the analysis behind it:
//! what each JAR declares about itself ([`metadata`]), how versions and
//! ranges compare ([`version`]), and what that adds up to ([`health`]).
//!
//! Read-only throughout. Nothing here moves, renames or deletes a file -
//! acting on a finding stays an explicit choice the operator makes.

pub mod health;
pub mod metadata;
pub mod version;

pub use health::{analyze, ModpackHealth};
pub use metadata::read_jar;
