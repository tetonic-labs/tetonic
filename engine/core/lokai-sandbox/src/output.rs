//! Bounded stdout/stderr collection with truncation and flood cutoff.

use crate::types::{ResourceLimits, SandboxError};

/// Drain both pipes through EOF, retaining bounded output while continuing to
/// consume overflow. The caller owns the runtime deadline and process cleanup.
#[cfg(any(unix, test))]
pub(crate) async fn drain_pipes(
    stdout: &mut (impl tokio::io::AsyncRead + Unpin),
    stderr: &mut (impl tokio::io::AsyncRead + Unpin),
    collector: &mut OutputCollector,
) -> Result<(), SandboxError> {
    use tokio::io::AsyncReadExt;
    let (mut out_open, mut err_open) = (true, true);
    let mut out = [0u8; 4096];
    let mut err = [0u8; 4096];
    while out_open || err_open {
        tokio::select! {
            read = stdout.read(&mut out), if out_open => {
                let n = read.map_err(|e| SandboxError::Io(e.to_string()))?;
                out_open = n != 0;
                if n != 0 { let _ = collector.push_stdout(&out[..n]); }
            }
            read = stderr.read(&mut err), if err_open => {
                let n = read.map_err(|e| SandboxError::Io(e.to_string()))?;
                err_open = n != 0;
                if n != 0 { let _ = collector.push_stderr(&err[..n]); }
            }
        }
    }
    Ok(())
}

pub struct OutputCollector {
    limits: ResourceLimits,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_cut: bool,
    stderr_cut: bool,
}

impl OutputCollector {
    pub fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_cut: false,
            stderr_cut: false,
        }
    }

    pub fn push_stdout(&mut self, chunk: &[u8]) -> Result<(), SandboxError> {
        Self::append(
            &mut self.stdout,
            &mut self.stdout_cut,
            chunk,
            self.limits.max_output_bytes_per_stream,
        )
    }

    pub fn push_stderr(&mut self, chunk: &[u8]) -> Result<(), SandboxError> {
        Self::append(
            &mut self.stderr,
            &mut self.stderr_cut,
            chunk,
            self.limits.max_output_bytes_per_stream,
        )
    }

    fn append(
        buf: &mut Vec<u8>,
        cut: &mut bool,
        chunk: &[u8],
        max: usize,
    ) -> Result<(), SandboxError> {
        if buf.len() >= max {
            *cut = true;
            return Err(SandboxError::Io("output limit exceeded".into()));
        }
        let room = max - buf.len();
        if chunk.len() > room {
            buf.extend_from_slice(&chunk[..room]);
            *cut = true;
            return Err(SandboxError::Io("output limit exceeded".into()));
        }
        buf.extend_from_slice(chunk);
        Ok(())
    }

    pub fn into_strings(self) -> (String, String, bool) {
        let trunc = self.stdout_cut || self.stderr_cut;
        (
            String::from_utf8_lossy(&self.stdout).into_owned(),
            String::from_utf8_lossy(&self.stderr).into_owned(),
            trunc,
        )
    }
}

pub fn truncate_for_display(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut cut = max;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}\n…[truncated {} bytes]", &s[..cut], s.len() - cut)
    }
}

#[cfg(test)]
mod pipe_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn early_stdout_eof_does_not_drop_stderr_tail_or_spin() {
        let (mut out_tx, mut out_rx) = tokio::io::duplex(32);
        let (mut err_tx, mut err_rx) = tokio::io::duplex(32);
        let producer = async move {
            out_tx.write_all(b"done").await.unwrap();
            drop(out_tx);
            tokio::task::yield_now().await;
            err_tx.write_all(&vec![b'e'; 65536]).await.unwrap();
        };
        let limits = ResourceLimits {
            max_output_bytes_per_stream: 70000,
            ..ResourceLimits::default()
        };
        let mut collector = OutputCollector::new(limits);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (_, drained) = tokio::join!(
                producer,
                drain_pipes(&mut out_rx, &mut err_rx, &mut collector)
            );
            drained.unwrap();
        })
        .await
        .unwrap();
        let (out, err, truncated) = collector.into_strings();
        assert_eq!(out, "done");
        assert_eq!(err.len(), 65536);
        assert!(!truncated);
    }
}
