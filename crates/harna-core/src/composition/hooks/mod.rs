//! Group `hooks`: enforcement that runs on its own — `setup` installs
//! and audits the hooks, `events` answers them (session-start,
//! post-edit, stop).

pub mod events;
pub mod setup;
