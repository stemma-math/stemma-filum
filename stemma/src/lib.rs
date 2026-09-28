//! The Stemma client.
//!
//! What a researcher installs on their own machine: the command line, the
//! daemon, the group repositories and the Typst and Lean tooling. The
//! library holds all of it and the binary only parses arguments, so the
//! desktop application and the tests use the same code.
//!
//! The modules fall into a few groups:
//!
//! - [`account`]: signing this machine in and out, and the credential it
//!   keeps.
//! - [`api`]: talking to an instance, and hearing what changes on it.
//! - [`mirror`]: keeping this machine's lines in step with the instance.
//! - [`job`]: launching agent jobs and carrying them out on this machine.
//! - [`machine`]: the daemon, the service it runs under, and setting this
//!   machine up and taking it apart.
//! - [`typeset`], [`lean`], [`git`] and [`build`]: the tooling that turns
//!   what is written into pages, checks and artefacts.
//! - The command modules ([`claims`], [`conflicts`], [`lines`],
//!   [`proposals`], [`vault`], [`node`], [`machines`], [`export`]): one
//!   per family of commands, each shared by the command line and the
//!   application.

pub mod account;
pub mod api;
pub mod build;
pub mod claims;
pub mod cli;
pub mod config;
pub mod conflicts;
pub mod export;
pub mod git;
pub mod home;
pub mod job;
pub mod lean;
pub mod lines;
pub mod machine;
pub mod machines;
pub mod mirror;
pub mod node;
pub mod proposals;
pub mod remedy;
pub mod repository;
pub mod storage;
pub mod typeset;
pub mod vault;
