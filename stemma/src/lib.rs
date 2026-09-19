//! The Stemma client.
//!
//! What a researcher installs on their own machine: the command line, and
//! in time the daemon, the repositories and the compilers. The library
//! holds all of it, and the binary beside it only parses arguments, so
//! that the desktop application and a test can use the same code.

pub mod agents;
pub mod api;
pub mod build;
pub mod cli;
pub mod config;
pub mod credential;
pub mod credentials;
pub mod daemon;
pub mod git;
pub mod home;
pub mod jobs;
pub mod node;
pub mod pin;
pub mod session;
pub mod sync;
pub mod work;
pub mod workspace;
