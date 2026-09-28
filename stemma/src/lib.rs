//! The Stemma client.
//!
//! What a researcher installs on their own machine: the command line, the
//! daemon, the group repositories and the Typst and Lean tooling. The
//! library holds all of it and the binary only parses arguments, so the
//! desktop application and the tests use the same code.

pub mod agents;
pub mod api;
pub mod build;
pub mod claims;
pub mod cli;
pub mod config;
pub mod conflicts;
pub mod credentials;
pub mod daemon;
pub mod events;
pub mod export;
pub mod git;
pub mod git_credential;
pub mod home;
pub mod job_api;
pub mod job_workspace;
pub mod jobs;
pub mod lean;
pub mod lines;
pub mod machines;
pub mod mirror;
pub mod node;
pub mod package;
pub mod pin;
pub mod program;
pub mod proposals;
pub mod reaching;
pub mod remedy;
pub mod render;
pub mod session;
pub mod setup;
pub mod storage;
pub mod uninstall;
pub mod vault;
pub mod work;
pub mod workspace;
