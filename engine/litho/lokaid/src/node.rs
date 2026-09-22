//! Worker `--node` modes (node-enrollment-v1, fabric ingress N0.2).

use anyhow::Result;
use tetonic_app::Application;

pub async fn run_enroll() -> Result<()> {
    Application::run_node_enroll().await
}

pub async fn run_serve() -> Result<()> {
    Application::run_node_serve().await
}
