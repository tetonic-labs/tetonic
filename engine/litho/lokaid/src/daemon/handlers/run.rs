use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) async fn run_snapshot(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: RunSnapshotParams = parse(params)?;
        let services = self.services()?;
        let snap = services
            .app
            .runs
            .inspect_run(lokai_app::commands::InspectRunCommand {
                run_id: p.run_id.clone(),
            })
            .await
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        let snapshot = serde_json::to_value(&snap)
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("{e}")))?;
        Ok(to_value(RunSnapshotResult {
            run_id: snap.run_id.to_string(),
            sequence: snap.sequence,
            state: format!("{:?}", snap.state).to_ascii_lowercase(),
            snapshot,
        }))
    }

    pub(in crate::daemon) async fn run_resume(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: RunResumeParams = parse(params)?;
        let services = self.services()?;
        let replay = services
            .app
            .runs
            .resume_events(lokai_app::commands::ResumeRunEventsCommand {
                run_id: p.run_id.clone(),
                after_sequence: p.after_sequence,
                limit: p.limit,
            })
            .await
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        match replay {
            Ok(events) => {
                let events = events
                    .into_iter()
                    .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
                    .collect();
                Ok(to_value(RunResumeResult {
                    run_id: p.run_id,
                    events,
                    gap: None,
                }))
            }
            Err(gap) => Ok(to_value(RunResumeResult {
                run_id: p.run_id,
                events: Vec::new(),
                gap: Some(RunResumeGap {
                    requested_after: gap.requested_after,
                    earliest_available: gap.earliest_available,
                    snapshot_sequence: gap.snapshot_sequence,
                    reason: format!("{:?}", gap.reason),
                }),
            })),
        }
    }

    pub(in crate::daemon) async fn run_cancel(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: RunCancelParams = parse(params)?;
        let services = self.services()?;
        services
            .app
            .runs
            .cancel_run(lokai_app::commands::CancelByRunCommand { run_id: p.run_id })
            .await
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        Ok(to_value(Canceled { canceled: true }))
    }
}
