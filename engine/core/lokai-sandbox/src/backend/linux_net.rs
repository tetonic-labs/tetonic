//! Linux OS network denial via user+network namespaces (H1-3).
//!
//! Unprivileged `CLONE_NEWUSER | CLONE_NEWNET` yields an empty netns: outbound
//! sockets fail without host root. Capability is advertised only when
//! unprivileged user namespaces are available.

use std::fs;
use std::io::Write;

use crate::types::NetworkPolicy;

/// True when this process can create an unprivileged user+net namespace.
pub fn network_denial_available() -> bool {
    if let Ok(v) = fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone") {
        if v.trim() == "0" {
            return false;
        }
    }
    // Probe: fork+unshare is heavy for every caps() call; assume available unless
    // the sysctl explicitly disables it (common enterprise lockdown).
    true
}

/// Apply DenyAll / AllowLoopback isolation in the child after fork, before exec.
///
/// Returns `Ok(())` on success. On failure the child should abort spawn.
pub fn apply_in_child(policy: &NetworkPolicy) -> std::io::Result<()> {
    match policy {
        NetworkPolicy::DenyAll | NetworkPolicy::AllowLoopback => {}
        NetworkPolicy::AllowDestinations(_) | NetworkPolicy::InheritBrokered => {
            return Ok(());
        }
    }

    use nix::sched::{unshare, CloneFlags};
    use nix::unistd::{getgid, getuid};

    unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET).map_err(std::io::Error::other)?;

    let uid = getuid().as_raw();
    let gid = getgid().as_raw();

    // Deny setgroups before writing gid_map (required on modern kernels).
    if let Ok(mut f) = fs::OpenOptions::new()
        .write(true)
        .open("/proc/self/setgroups")
    {
        let _ = f.write_all(b"deny");
    }
    fs::write("/proc/self/uid_map", format!("0 {uid} 1"))?;
    fs::write("/proc/self/gid_map", format!("0 {gid} 1"))?;

    if matches!(policy, NetworkPolicy::AllowLoopback) {
        bring_up_loopback()?;
    }
    // DenyAll: leave all interfaces down / unaddressed — no route off-box.
    Ok(())
}

fn bring_up_loopback() -> std::io::Result<()> {
    // Best-effort: `ip link set lo up` if present; empty netns without lo is still
    // deny-all for outbound. AllowLoopback is best-effort.
    let status = std::process::Command::new("ip")
        .args(["link", "set", "lo", "up"])
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Ok(()), // still isolated; loopback may be unavailable
    }
}
