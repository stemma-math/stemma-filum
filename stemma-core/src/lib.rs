//! Everything the Stemma client and server both need to agree on.
//!
//! This crate holds identifiers, the domain vocabulary and the versioned
//! protocol. It performs no input or output and knows about no storage: the
//! client links it to write mathematics, the server links it to serve them,
//! and neither depends on the other.

#[macro_use]
mod spelled;

pub mod access;
pub mod actor;
pub mod agent;
pub mod artefact;
pub mod claim;
pub mod descent;
pub mod diff;
pub mod document;
pub mod fact;
pub mod id;
pub mod job;
pub mod line;
pub mod machine;
pub mod model;
pub mod notation;
pub mod proposal;
pub mod protocol;
pub mod reconcile;
pub mod render;
pub mod repository;
pub mod source;
pub mod vault;

pub use protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION, ProtocolVersion};
