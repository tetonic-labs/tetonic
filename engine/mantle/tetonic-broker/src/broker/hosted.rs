//! Hosted calls are explicit outbound HTTP from a local executor, not fabric
//! workers. Reserve local orchestration capacity; disclosure/endpoint/secret
//! checks remain inside HostedChatProvider and EgressGuard on every call.
use super::*;
use tetonic_inference::hosted::HostedChatProvider;

struct Release<'a> {
    broker: &'a DefaultComputeBroker,
    attempt: AttemptId,
    finished: bool,
}
impl Drop for Release<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.broker.finish_attempt(
                &self.attempt,
                false,
                Some("hosted call interrupted".into()),
            );
        }
    }
}

impl DefaultComputeBroker {
    pub async fn chat_hosted_admitted(
        &self,
        provider: &HostedChatProvider,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let mut request = compute_request_from_chat(&req);
        // This reservation covers the local HTTP orchestration, not a local GPU.
        request.resource_request.vram_bytes = 0;
        request.resource_request.gpu_devices.clear();
        request.input_digest = tetonic_domain::ContentDigest::new("hosted-explicit");
        let failure = |message: &str| InferenceError::Provider(message.into());
        let handle = self
            .submit(request.clone())
            .await
            .map_err(|_| failure("hosted inference admission failed"))?;
        if !matches!(handle.status, ComputeStatus::Reserved) {
            self.finish_attempt(
                &request.attempt_id,
                false,
                Some("hosted admission unavailable".into()),
            );
            return Err(failure(
                "hosted inference capacity unavailable; retry later",
            ));
        }
        let mut release = Release {
            broker: self,
            attempt: request.attempt_id.clone(),
            finished: false,
        };
        self.gate_dispatch(&request)
            .map_err(|_| failure("hosted dispatch authorization failed"))?;
        if let Some(reservation) = self.budgets.get_by_attempt(&request.attempt_id) {
            self.budgets
                .transition(&reservation.reservation_id, ReservationState::Running)
                .map_err(|_| failure("hosted inference reservation unavailable"))?;
        }
        let result = provider.chat(req, on_token).await;
        self.finish_attempt(
            &request.attempt_id,
            result.is_ok(),
            result
                .as_ref()
                .err()
                .map(|_| "hosted inference failed".into()),
        );
        release.finished = true;
        result
    }
}
