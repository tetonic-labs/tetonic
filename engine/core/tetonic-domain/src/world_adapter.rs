//! [`WorldAdapter`] — the contract any environment must implement to host
//! Tetonic agents in continuous (live-world) mode.
//!
//! Implementing this trait is the *only* thing a world needs to do.
//! The agent runtime drives the perception → brain → action cycle;
//! the adapter provides environment-specific translation on both sides.
//!
//! # World-agnostic agents
//!
//! The adapter is what makes an agent world-agnostic. The agent definition —
//! identity, goals, brain, memory — never references a specific environment.
//! Deploying an agent to a new world means supplying a new adapter:
//!
//! ```text
//! Agent (universal) + WorldAdapter (village)   → village agent
//! Agent (universal) + WorldAdapter (CI/CD)     → autonomous dev agent
//! Agent (universal) + WorldAdapter (robotics)  → physical robot controller
//! ```
//!
//! # Tick rate
//!
//! The adapter controls the cadence. Send perceptions as fast as your world
//! ticks — sub-second for games and robotics, seconds for simulations,
//! minutes for slow async environments. The brain's System 1 layer handles
//! whatever rate it receives; System 2 runs on its own slower clock.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{ActionResult, Perception, WorldAction, WorldError};

// ── PerceptionReceiver ────────────────────────────────────────────────────────

/// A channel endpoint the agent runtime reads perceptions from.
///
/// The adapter holds the [`mpsc::Sender`] side and pushes ticks.
/// The agent runtime holds the `PerceptionReceiver` and drives the brain loop.
pub type PerceptionReceiver = mpsc::Receiver<Perception>;
pub type PerceptionSender = mpsc::Sender<Perception>;

// ── WorldAdapter ──────────────────────────────────────────────────────────────

/// The contract any environment must implement to host a Tetonic agent.
///
/// # Implementation guidance
///
/// - `perception_stream` should start pushing ticks immediately. The agent
///   runtime will drain the receiver as fast as the brain can process them.
/// - For latest-value semantics (e.g. fast game worlds where stale ticks are
///   worse than skipped ticks), use a channel capacity of 1 and `try_send`
///   with `replace` semantics instead of queuing.
/// - `execute` should be idempotent where possible; the brain may issue
///   duplicate actions if it retries after a timeout.
#[async_trait]
pub trait WorldAdapter: Send + Sync {
    /// Open a perception stream at the world's natural tick rate.
    ///
    /// Returns a sender the adapter drives and a receiver the agent reads.
    /// The adapter spawns a background task to push ticks; the runtime
    /// drives the brain loop from the receiver.
    fn open(&self) -> (PerceptionSender, PerceptionReceiver);

    /// Execute an action the agent's brain decided on.
    ///
    /// Returns feedback that may be injected into the next perception's events
    /// so the agent can observe whether its action had the intended effect.
    async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError>;

    /// Human-readable description of this world for logging and tracing.
    fn describe(&self) -> &str;
}
