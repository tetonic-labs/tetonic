use super::*;

pub(super) async fn save(
    state: &State,
    body: &[u8],
) -> Result<serde_json::Value, tetonic_app::errors::AppError> {
    let payload = serde_json::from_slice::<tetonic_app::work::SaveToolPack>(body)
        .map_err(|_| tetonic_app::errors::AppError::InvalidRequest("Invalid tool pack.".into()))?;
    state
        .workspace
        .save_tool_pack(payload)
        .await
        .map(|v| serde_json::json!(v))
}
