use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;

/// Initializes application-level logging (ModForge's own errors/diagnostics -
/// separate from a Minecraft instance's `logs/`, which the server module
/// writes independently).
///
/// Returns a [`WorkerGuard`] that must be kept alive for the process
/// lifetime (dropping it stops the background writer thread), so the caller
/// should hold onto it in a variable that outlives `tauri::Builder::run`.
pub fn init(logs_dir: &Path) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(logs_dir, "modforge.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    let stdout_layer = tracing_subscriber::fmt::layer();

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(stdout_layer)
        .with(file_layer)
        .init();

    guard
}
