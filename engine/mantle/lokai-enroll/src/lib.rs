//! Worker enrollment — shared-secret proof + pinned keys (N0.1).
//!
//! See `docs/implementation/contracts/node-enrollment-v1.md`.

mod bind_policy;
mod code;
mod crypto;
mod egress;
mod handshake;
mod http;
mod resolve;
mod server;
mod tls;

pub use bind_policy::{is_loopback_bind, plaintext_enrollment_permitted};
pub use code::{
    advertise_host, decode_enrollment_code, encode_enrollment_code, EnrollmentCode,
    DEFAULT_ENROLL_PORT, DEFAULT_ENROLL_TTL, DEFAULT_FABRIC_PORT, ENROLL_CODE_PREFIX,
};
pub use crypto::{KeyPair, PublicKeyBytes};
pub use egress::allow_fabric_workers;
pub use handshake::{complete_enrollment, EnrollCompleteRequest, EnrollCompleteResponse};
pub use server::{run_enrollment_server, EnrollmentServerConfig, EnrollmentServerOutcome};
