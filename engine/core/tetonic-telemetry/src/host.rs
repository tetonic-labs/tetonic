//! Operator-selected, sanitized host diagnostics. Installation is a binary concern.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing_subscriber::{filter::LevelFilter, prelude::*};

use crate::sanitization::TraceFormatter;
use crate::{DiagnosticMode, TraceStorageBudget};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    #[default]
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Error => Self::ERROR,
            LogLevel::Warn => Self::WARN,
            LogLevel::Info => Self::INFO,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Trace => Self::TRACE,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub stderr: bool,
    /// Optional daily-rotated, sanitized JSON-lines files.
    pub directory: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Warn,
            stderr: true,
            directory: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TraceConfig {
    pub sample_rate: f64,
    /// Budget for emitted trace bytes in this process, not a filesystem quota.
    pub byte_budget: u64,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            sample_rate: 1.0,
            byte_budget: TraceStorageBudget::defaults().trace_quota_bytes,
        }
    }
}

impl TraceConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.sample_rate.is_finite() || !(0.0..=1.0).contains(&self.sample_rate) {
            return Err("telemetry.sample_rate must be between 0 and 1");
        }
        if self.byte_budget == 0 {
            return Err("telemetry.byte_budget must be positive");
        }
        Ok(())
    }
}

/// Keep alive until the command finishes so buffered diagnostics are flushed.
pub struct HostDiagnosticsGuard {
    _writer: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Builds a subscriber without installing process-global state. All sinks use
/// the existing safe formatter; raw prompts/process output are never enabled here.
pub fn host_subscriber(
    logging: &LoggingConfig,
) -> Result<(impl tracing::Subscriber + Send + Sync, HostDiagnosticsGuard), std::io::Error> {
    let (writer, guard) = if let Some(directory) = &logging.directory {
        let appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("tetonic")
            .filename_suffix("jsonl")
            .max_log_files(7)
            .build(directory)
            .map_err(std::io::Error::other)?;
        let (writer, guard) = tracing_appender::non_blocking(appender);
        (Some(writer), Some(guard))
    } else {
        (None, None)
    };
    let file = writer.map(|writer| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(writer)
            .event_format(TraceFormatter::new(DiagnosticMode::Safe))
            .with_filter(LevelFilter::from(logging.level))
    });
    let stderr = logging.stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::io::stderr)
            .event_format(TraceFormatter::new(DiagnosticMode::Safe))
            .with_filter(LevelFilter::from(logging.level))
    });
    Ok((
        tracing_subscriber::registry().with(file).with(stderr),
        HostDiagnosticsGuard { _writer: guard },
    ))
}

/// Binary startup only. Library composition must leave subscriber ownership to
/// its embedding process. Failure is explicit rather than silently dropping logs.
pub fn install_host_diagnostics(
    logging: &LoggingConfig,
    telemetry: &TraceConfig,
) -> Result<HostDiagnosticsGuard, Box<dyn std::error::Error + Send + Sync>> {
    telemetry.validate()?;
    let (subscriber, guard) = host_subscriber(logging)?;
    tracing::subscriber::set_global_default(subscriber)?;
    let defaults = TraceStorageBudget::defaults();
    crate::configure_trace_gate(
        TraceStorageBudget::from_quotas(
            telemetry
                .byte_budget
                .saturating_add(defaults.reserved_recovery_bytes),
            telemetry.byte_budget,
            defaults.reserved_recovery_bytes,
        ),
        telemetry.sample_rate,
    );
    Ok(guard)
}
