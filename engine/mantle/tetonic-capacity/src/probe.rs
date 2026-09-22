//! Live Ollama placement probe (`/api/ps` structured fields + NVML).

use chrono::Utc;
use serde_json::Value;

use crate::profile::ObservedPlacement;

/// GPU layer residency [0, 100] from structured `/api/ps` `size_vram` / `size`.
pub fn gpu_residency_pct_from_ps(size: u64, size_vram: u64) -> Option<f32> {
    if size == 0 {
        return None;
    }
    Some((100.0 * size_vram as f64 / size as f64).min(100.0) as f32)
}

/// Parse Ollama `/api/ps` JSON into placement snapshot (structured metrics only).
pub fn parse_ps_response(v: &Value) -> Option<ObservedPlacement> {
    let models = v.get("models")?.as_array()?;
    // An unqualified observation is only meaningful for a single runner.
    if models.len() != 1 {
        return None;
    }
    parse_model(&models[0])
}

/// Placement of the requested runner, never a different resident model.
pub fn parse_ps_response_for_model(v: &Value, model: &str) -> Option<ObservedPlacement> {
    let models = v.get("models")?.as_array()?;
    let m = models.iter().find(|m| {
        m.get("name")
            .or_else(|| m.get("model"))
            .and_then(Value::as_str)
            .is_some_and(|name| tetonic_inference::ollama_model_matches(name, model))
    })?;
    parse_model(m)
}

fn parse_model(m: &Value) -> Option<ObservedPlacement> {
    let size = m.get("size").and_then(|x| x.as_u64()).unwrap_or(0);
    let size_vram = m.get("size_vram").and_then(|x| x.as_u64()).unwrap_or(0);
    let processor_split = m
        .get("processor")
        .and_then(|p| p.as_str())
        .map(String::from);
    let name = m
        .get("name")
        .or_else(|| m.get("model"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    let gpu_processor_pct = gpu_residency_pct_from_ps(size, size_vram);
    let vram_from_ps = (size_vram / (1024 * 1024)) as u32;
    // Whole-device NVML usage includes other models and applications. Do not
    // substitute it for this runner's allocation (or mix host and remote GPUs).
    let vram_used_mb = vram_from_ps;

    Some(ObservedPlacement {
        gpu_processor_pct,
        processor_split,
        vram_used_mb,
        resident_model: name,
        load_wall_s: 0.0,
        measured_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selects_requested_model_without_using_first_runner_or_device_total() {
        let v = json!({"models":[
            {"name":"other:latest", "size":100, "size_vram":100},
            {"name":"wanted:latest", "size":104857600, "size_vram":20971520}
        ]});
        let observed = parse_ps_response_for_model(&v, "wanted").unwrap();
        assert_eq!(observed.gpu_processor_pct, Some(20.0));
        assert_eq!(observed.vram_used_mb, 20);
        assert_eq!(observed.resident_model, "wanted:latest");
        assert!(parse_ps_response_for_model(&v, "missing").is_none());
        assert!(parse_ps_response_for_model(&v, "want").is_none());
        assert!(parse_ps_response(&v).is_none());
    }

    #[test]
    fn residency_from_structured_ps_fields() {
        let size = 6_591_830_464u64;
        let size_vram = 5_333_539_264u64;
        let pct = gpu_residency_pct_from_ps(size, size_vram).unwrap();
        assert!(pct > 80.0 && pct <= 100.0);
    }

    #[test]
    fn parses_ps_structured_json() {
        let v = json!({
            "models": [{
                "name": "qwen3.6-estate",
                "size": 6591830464_i64,
                "size_vram": 5333539264_i64,
                "processor": "76%/24%"
            }]
        });
        let o = parse_ps_response(&v).unwrap();
        assert!(o.gpu_processor_pct.is_some());
        assert!(o.gpu_processor_pct.unwrap() > 80.0);
        assert_eq!(o.processor_split.as_deref(), Some("76%/24%"));
        assert_eq!(o.resident_model, "qwen3.6-estate");
    }
}
