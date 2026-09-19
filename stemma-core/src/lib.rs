//! Everything the Stemma client and server both need to agree on.
//!
//! This crate holds identifiers, the domain vocabulary and the versioned
//! protocol. It performs no input or output and knows about no storage: the
//! client links it to write mathematics, the server links it to serve them,
//! and neither depends on the other.

pub mod access;
pub mod actor;
pub mod agent;
pub mod artefact;
pub mod fact;
pub mod id;
pub mod line;
pub mod model;
pub mod protocol;
pub mod repository;
pub mod source;

pub use protocol::{PROTOCOL_HEADER, PROTOCOL_VERSION, ProtocolVersion};
