//! The Stemma server.
//!
//! The library holds everything the instance is: its configuration, its
//! database, the web it serves and the operations an administrator runs.
//! The binary beside it only parses arguments and calls in here, so that
//! every part of the server can be exercised from a test.

pub mod admin;
pub mod artefacts;
pub mod check;
pub mod cli;
pub mod config;
pub mod db;
pub mod git;
pub mod install;
pub mod password;
pub mod repositories;
pub mod serve;
pub mod service;
pub mod setup;
pub mod status;
pub mod store;
pub mod system;
pub mod tokens;
pub mod web;
