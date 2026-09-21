//! The Stemma client.
//!
//! What a researcher installs on their own machine: the command line, and
//! in time the daemon, the repositories and the compilers. The library
//! holds all of it, and the binary beside it only parses arguments, so
//! that the desktop application and a test can use the same code.

pub mod agents;
pub mod api;
pub mod build;
pub mod claims;
pub mod cli;
pub mod config;
pub mod conflicts;
pub mod credential;
pub mod credentials;
pub mod daemon;
pub mod git;
pub mod home;
pub mod jobs;
pub mod lean;
pub mod lines;
pub mod machines;
pub mod node;
pub mod notation;
pub mod pin;
pub mod program;
pub mod proposals;
pub mod session;
pub mod storage;
pub mod sync;
pub mod vault;
pub mod work;
pub mod workspace;
