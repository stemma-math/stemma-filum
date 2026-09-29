//! Agent jobs: asking for one, and doing one on this machine.
//!
//! A job is only ever started because a person asked for it. The
//! launcher puts it in the instance's queue; the daemon of the machine it
//! was asked of takes it, prepares a workspace, runs the agent in it, and
//! reports what came of it.

pub mod agents;
pub mod answer;
pub mod launch;
pub mod work;
pub mod workspace;
