//! Reload fabric allow rules from persisted worker rows.

use std::net::IpAddr;

use tetonic_egress::EgressGuard;

/// Apply permanent fabric egress allow rules for enrolled workers.
pub fn allow_fabric_workers(
    guard: &EgressGuard,
    workers: impl IntoIterator<Item = (String, IpAddr, u16)>,
) {
    for (label, ip, port) in workers {
        guard.allow_node(label, ip, Some(port));
    }
}
