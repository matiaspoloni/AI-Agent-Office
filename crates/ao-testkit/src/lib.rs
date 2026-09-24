//! Test support shared by provider crates. Never used by the shipped app.
//!
//! * [`fixtures`] — load provider payload fixtures and compare the produced
//!   [`AgentEvent`](ao_core::AgentEvent)s against expected JSON (subset match,
//!   ignoring `eventId`/`timestamp`).
//! * [`bins`] — build and locate helper binaries (fake provider CLIs, the hook
//!   relay) from inside tests, without real accounts or installs.

pub mod bins;
pub mod fixtures;
