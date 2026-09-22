//! Hardware detection (v0) — NVML via nvidia-smi + sysinfo.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::Command;

use sysinfo::System;

use crate::profile::{GpuInfo, GpuRole, HardwareSnapshot};

pub fn detect_hardware(ollama_version: Option<String>) -> HardwareSnapshot {
    let gpus = detect_nvidia_gpus();
    let mut sys = System::new();
    sys.refresh_all();
    let cpu_cores = sys.cpus().len().max(1) as u32;
    let ram_mb = sys.total_memory() / (1024 * 1024);
    let os = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let fingerprint = compute_fingerprint(&gpus, cpu_cores, ram_mb, &os);
    HardwareSnapshot {
        fingerprint,
        gpus,
        cpu_cores,
        ram_mb,
        os,
        ollama_version,
    }
}

pub fn compute_fingerprint(gpus: &[GpuInfo], cpu_cores: u32, ram_mb: u64, os: &str) -> String {
    let mut h = DefaultHasher::new();
    for g in gpus {
        g.uuid.hash(&mut h);
        g.name.hash(&mut h);
        g.vram_total_mb.hash(&mut h);
        g.bus_id.hash(&mut h);
    }
    cpu_cores.hash(&mut h);
    ram_mb.hash(&mut h);
    os.hash(&mut h);
    format!("fp_{:016x}", h.finish())
}

pub fn hardware_summary(snapshot: &HardwareSnapshot) -> String {
    let compute: Vec<_> = snapshot
        .gpus
        .iter()
        .filter(|g| g.role == GpuRole::Compute || g.role == GpuRole::Unknown)
        .collect();
    if compute.is_empty() && snapshot.gpus.is_empty() {
        return format!(
            "CPU {} cores, {} GB RAM",
            snapshot.cpu_cores,
            snapshot.ram_mb / 1024
        );
    }
    compute
        .iter()
        .map(|g| format!("{} {}MB", g.name, g.vram_total_mb))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn detect_nvidia_gpus() -> Vec<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=uuid,name,memory.total,pci.bus_id",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(out) = output else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut gpus = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 2 {
            continue;
        }
        let uuid = parts.first().map(|s| (*s).to_string());
        let name = parts.get(1).unwrap_or(&"GPU").to_string();
        let vram = parts
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let bus_id = parts.get(3).map(|s| (*s).to_string());
        let role = infer_gpu_role(&name, gpus.len());
        gpus.push(GpuInfo {
            name,
            uuid,
            bus_id,
            vram_total_mb: vram,
            role,
        });
    }
    gpus
}

fn infer_gpu_role(name: &str, index: usize) -> GpuRole {
    let lower = name.to_lowercase();
    if lower.contains("tesla")
        || lower.contains("a100")
        || lower.contains("h100")
        || lower.contains("p40")
        || lower.contains("p100")
        || lower.contains("v100")
        || lower.contains("rtx 40")
        || lower.contains("rtx 30")
    {
        return GpuRole::Compute;
    }
    if lower.contains("geforce") && (lower.contains("970") || lower.contains("1050")) {
        return GpuRole::Display;
    }
    if index == 0 {
        GpuRole::Compute
    } else {
        GpuRole::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_stable() {
        let gpus = vec![GpuInfo {
            name: "Tesla P40".into(),
            uuid: Some("GPU-1".into()),
            bus_id: Some("03:00.0".into()),
            vram_total_mb: 24_576,
            role: GpuRole::Compute,
        }];
        let a = compute_fingerprint(&gpus, 8, 32_768, "windows-x86_64");
        let b = compute_fingerprint(&gpus, 8, 32_768, "windows-x86_64");
        assert_eq!(a, b);
    }
}
