//! Rusty Host core library.
//!
//! All logic lives here; bindings (`rh-cli`, `rh-gui`, `rh-term`) only present it
//! (PRD §4.1). Nothing in this crate prints to stdout.

pub mod component;
pub mod fetch;
pub mod hosts;
pub mod ports;
pub mod proc;
pub mod project;
pub mod recipe;
pub mod resolve;
pub mod settings;
pub mod template;
pub mod unpack;
pub mod vhost;
pub mod workspace;
