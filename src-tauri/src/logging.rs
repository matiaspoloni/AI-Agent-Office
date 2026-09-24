//! Structured logging to rolling files:
//! * `app.log` — application log,
//! * `providers.log` — adapter/protocol traffic (targets starting with `provider`).
//!
//! Level: `AGENT_OFFICE_LOG` (e.g. `debug`, `info,provider=debug`), default `info`.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::{filter_fn, EnvFilter};
use tracing_subscriber::prelude::*;

pub struct LogGuards {
    _guards: Vec<WorkerGuard>,
}

fn is_provider_target(target: &str) -> bool {
    target.starts_with("provider") || target.starts_with("ao_provider")
}

pub fn init(log_dir: &Path) -> LogGuards {
    let _ = std::fs::create_dir_all(log_dir);
    let (app_writer, app_guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily(log_dir, "app.log"));
    let (provider_writer, provider_guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily(log_dir, "providers.log"));

    let level =
        || EnvFilter::try_from_env("AGENT_OFFICE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let app_layer = tracing_subscriber::fmt::layer()
        .with_writer(app_writer)
        .with_ansi(false)
        .with_target(true)
        .with_filter(level())
        .with_filter(filter_fn(|meta| !is_provider_target(meta.target())));

    let provider_layer = tracing_subscriber::fmt::layer()
        .with_writer(provider_writer)
        .with_ansi(false)
        .with_filter(level())
        .with_filter(filter_fn(|meta| is_provider_target(meta.target())));

    let console_layer = cfg!(debug_assertions).then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_filter(level())
    });

    let _ = tracing_subscriber::registry()
        .with(app_layer)
        .with(provider_layer)
        .with(console_layer)
        .try_init();

    LogGuards {
        _guards: vec![app_guard, provider_guard],
    }
}
