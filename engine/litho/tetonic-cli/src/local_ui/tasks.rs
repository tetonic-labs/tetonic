//! Task inspection and control transport. Parent routing authenticates first.
use super::*;

pub(super) async fn handle(
    state: &State,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let result = if request.method() == hyper::Method::POST {
        if let Some(id) = id.strip_suffix("/draft") {
            let body = match tokio::time::timeout(
                Duration::from_secs(5),
                Limited::new(request.into_body(), 8192).collect(),
            )
            .await
            {
                Ok(Ok(body)) => body.to_bytes(),
                _ => return error(StatusCode::BAD_REQUEST, "Incomplete draft request."),
            };
            let Ok(payload) =
                serde_json::from_slice::<tetonic_app::local_workspace::ReadWorkDraft>(&body)
            else {
                return error(StatusCode::BAD_REQUEST, "Invalid draft request.");
            };
            state
                .workspace
                .read_work_draft(id, payload)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else if let Some(id) = id.strip_suffix("/cancel") {
            state
                .workspace
                .cancel(id)
                .await
                .map(|v| serde_json::to_value(v).unwrap_or_default())
        } else {
            return error(StatusCode::NOT_FOUND, "Unknown action.");
        }
    } else if request.method() == hyper::Method::GET {
        state
            .workspace
            .task(id)
            .await
            .map(|v| serde_json::to_value(v).unwrap_or_default())
    } else {
        return error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method.");
    };
    match result {
        Ok(value) => json(StatusCode::OK, value),
        Err(e) => application_error(&e),
    }
}
