//! The Stemma client.
//!
//! What a researcher installs on their own machine: the command line, and
//! in time the daemon, the repositories and the compilers. The library
//! holds all of it, and the binary beside it only parses arguments, so
//! that the desktop application and a test can use the same code.

pub mod api;
pub mod cli;
pub mod credentials;
pub mod home;
pub mod session;
