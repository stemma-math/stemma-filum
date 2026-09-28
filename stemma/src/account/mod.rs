//! This machine's account on an instance.
//!
//! Signing in hands this machine a credential; everything else that talks
//! to the instance, git included, goes through that one credential.

pub mod credentials;
pub mod git_credential;
pub mod session;
