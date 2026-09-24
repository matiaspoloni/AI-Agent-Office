//! Agent Office core.
//!
//! This crate has no OS, UI or database dependencies. It defines:
//! * the unified event model ([`event`]),
//! * capability declarations ([`capabilities`]),
//! * the provider adapter contract and registry ([`provider`], [`registry`]),
//! * the ingest pipeline, session/agent state reducer and UI batching
//!   ([`pipeline`], [`world`], [`batch`]).

pub mod activity;
pub mod batch;
pub mod capabilities;
pub mod event;
pub mod ids;
pub mod pipeline;
pub mod provider;
pub mod registry;
pub mod sanitize;
pub mod time;
pub mod world;

pub use activity::Activity;
pub use batch::{Batcher, UiBatch};
pub use capabilities::{Capabilities, CapabilityProfile, ImplementedFeatures, Support};
pub use event::{AgentEvent, EventKind, EventSource, SessionMode, ToolCategory};
pub use ids::*;
pub use pipeline::{Ingest, Pipeline, ProjectRoot};
pub use provider::{
    AdapterContext, EventSink, ExternalSessionInfo, InstallationInfo, IntegrationState,
    IntegrationStatus, LaunchRequest, PermissionDecision, ProviderAdapter, ProviderDescriptor,
    ProviderError, SessionHandle, StopMode,
};
pub use registry::{ProviderInfo, ProviderRegistry};
pub use sanitize::SanitizeLimits;
pub use world::{AgentState, SessionState, SessionStatus, WorldSnapshot, WorldState};
