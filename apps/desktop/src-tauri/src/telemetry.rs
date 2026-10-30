//! Redacted telemetry (FR-016, ADR-008; review findings F-10/F-11).
//!
//! Primary rule: secret values never enter spans/events by construction.
//! This module adds defense-in-depth: a formatter that rewrites fields with
//! sensitive names to `[REDACTED]` before they reach any sink, and sweep
//! tests that assert seeded key material never appears in output.
//!
//! Known limit (documented): the field-name layer cannot catch secret
//! material embedded in free-text messages — the sweep tests are the guard
//! there. Log files live under the OS per-user app-data/logs directory,
//! rotate daily, and files older than 14 days are pruned at startup.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(test)]
use std::sync::{Arc, Mutex};

use tracing::Event;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

/// Field names whose values are never written to any sink.
const SENSITIVE_FIELDS: &[&str] = &[
    "secret",
    "token",
    "authorization",
    "api_key",
    "password",
    "credential",
];

const LOG_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Collector visitor: captures event fields as strings for redacted output.
#[derive(Default)]
struct CollectingVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for CollectingVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.fields
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

fn is_sensitive(name: &str) -> bool {
    SENSITIVE_FIELDS.contains(&name)
}

/// Formatter that redacts sensitive fields and never emits bodies.
#[derive(Debug)]
pub struct RedactingFormatter;

impl<S, N> FormatEvent<S, N> for RedactingFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visit = CollectingVisitor::default();
        event.record(&mut visit);
        write!(
            writer,
            "{} {:>5} ",
            chrono::Utc::now().to_rfc3339(),
            event.metadata().level()
        )?;
        if let Some(message) = &visit.message {
            write!(writer, "{message} ")?;
        }
        for (name, value) in &visit.fields {
            if is_sensitive(name) {
                write!(writer, "{name}=[REDACTED] ")?;
            } else {
                write!(writer, "{name}={value} ")?;
            }
        }
        writeln!(writer)
    }
}

/// Resolves the per-user log directory (`%APPDATA%/<ident>/logs` on Windows,
/// `~/Library/Logs/<ident>` on macOS — matches Tauri's app-data base).
#[must_use]
pub fn log_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("com.aperiostudios.shahid-airouter").join("logs"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library/Logs/com.aperiostudios.shahid-airouter"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

/// Deletes rotated log files older than the retention window. Best-effort:
/// failures are logged and ignored.
pub fn prune_old_logs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now() - LOG_RETENTION;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let expired = meta
            .modified()
            .ok()
            .is_some_and(|modified| modified < cutoff);
        if expired {
            tracing::debug!(file = %entry.path().display(), "pruning old log file");
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Captured in-memory sink for sweep tests (test-only; the release build has
/// no such sink).
#[cfg(test)]
#[derive(Clone, Default)]
pub struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

#[cfg(test)]
impl CapturedLogs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
    }
}

#[cfg(test)]
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
    type Writer = CapturedWriter;

    fn make_writer(&'a self) -> Self::Writer {
        CapturedWriter(self.0.clone())
    }
}

#[cfg(test)]
pub struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

#[cfg(test)]
impl std::io::Write for CapturedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Installs the global subscriber: redacting formatter to stderr, plus a
/// daily-rotating file sink when the log dir is available. One EnvFilter
/// (default INFO) governs the whole registry. Returns the file writer guard
/// that must be kept alive for the process.
#[must_use]
pub fn init() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    match log_dir().and_then(|dir| {
        fs::create_dir_all(&dir).ok()?;
        prune_old_logs(&dir);
        let appender = tracing_appender::rolling::daily(&dir, "shahid-ai-router.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        Some((
            tracing_subscriber::fmt::layer()
                .event_format(RedactingFormatter)
                .with_writer(writer),
            guard,
        ))
    }) {
        Some((file_layer, guard)) => {
            let stderr = tracing_subscriber::fmt::layer()
                .event_format(RedactingFormatter)
                .with_writer(std::io::stderr);
            tracing_subscriber::registry()
                .with(file_layer)
                .with(stderr)
                .with(filter)
                .init();
            Some(guard)
        }
        None => {
            let stderr = tracing_subscriber::fmt::layer()
                .event_format(RedactingFormatter)
                .with_writer(std::io::stderr);
            tracing_subscriber::registry()
                .with(stderr)
                .with(filter)
                .init();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CapturedLogs, RedactingFormatter};
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;

    /// Builds a scoped subscriber so parallel tests never fight over the
    /// global default (and never capture nothing).
    fn scoped(sink: &CapturedLogs) -> impl tracing::Subscriber {
        tracing_subscriber::registry().with(
            fmt::layer()
                .event_format(RedactingFormatter)
                .with_writer(sink.clone())
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
        )
    }

    fn capture_log(event: impl FnOnce()) -> String {
        let sink = CapturedLogs::new();
        tracing::subscriber::with_default(scoped(&sink), event);
        sink.contents()
    }

    #[test]
    fn sensitive_fields_are_redacted_in_output() {
        let out = capture_log(|| {
            tracing::info!(
                secret = "sk-live-SECRETVALUE1",
                token = "TOKENVALUE2",
                api_key = "KEYVALUE3",
                model = "mock-large",
                "outbound request"
            );
        });
        assert!(!out.contains("SECRETVALUE1"), "secret leaked: {out}");
        assert!(!out.contains("TOKENVALUE2"), "token leaked: {out}");
        assert!(!out.contains("KEYVALUE3"), "api_key leaked: {out}");
        assert!(out.contains("[REDACTED]"));
        assert!(out.contains("model=mock-large"), "non-sensitive field lost");
        assert!(out.contains("outbound request"), "message lost");
    }

    #[test]
    fn non_sensitive_events_pass_through() {
        let out = capture_log(|| {
            tracing::info!(addr = "127.0.0.1:8787", "gateway started");
        });
        assert!(out.contains("gateway started"));
        assert!(out.contains("addr=127.0.0.1:8787"));
        assert!(!out.contains("[REDACTED]"));
    }

    /// F-11/SEC2-05: the on-disk file sink path (not just the in-memory
    /// writer) must contain no secret material.
    #[test]
    fn file_sink_contains_no_secret_material() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sweep.log");
        let sink = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(
            fmt::layer()
                .event_format(RedactingFormatter)
                .with_writer(std::sync::Mutex::new(sink))
                .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(api_key = "sk-file-SECRETVALUE9", provider = "mock", "call");
        });
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            !contents.contains("SECRETVALUE9"),
            "secret reached the file sink: {contents}"
        );
        assert!(contents.contains("provider=mock"));
        assert!(contents.contains("[REDACTED]"));
    }

    /// F-10: the token-reveal path must never log the token value. Runs the
    /// real AppState.ensure_token flow against the in-memory store and a
    /// memory DB while capturing logs.
    #[test]
    fn ensure_token_never_logs_token_material() {
        use crate::state::AppState;
        use sair_persistence::Db;
        use sair_vault::InMemorySecretStore;
        use std::sync::Arc;

        let sink = CapturedLogs::new();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        tracing::subscriber::with_default(scoped(&sink), || {
            rt.block_on(async {
                let db = Db::open("sqlite::memory:").await.unwrap();
                db.run_migrations().await.unwrap();
                let state = AppState::new(db, Arc::new(InMemorySecretStore::new()));

                let token = state.ensure_token().await.unwrap();
                let token_str = String::from_utf8(token.clone()).unwrap();
                assert_eq!(token.len(), 43);
                // Reveal path exercises the same code path as the IPC command.
                let revealed = state.reveal_token().await.unwrap();
                assert_eq!(revealed, token_str);

                let out = sink.contents();
                assert!(
                    !out.contains(&token_str),
                    "gateway token material appeared in logs: {out}"
                );
            });
        });
    }
}
