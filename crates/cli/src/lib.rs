//! Upleft's port of Downright's `drdownright` library target: the logic of
//! the `down` command-line tool, shared with the app so the Settings toggle
//! and the CLI can never disagree about what is installed.
//!
//! One module per Swift file, same names in snake_case. Swift `public` and
//! `internal` declarations are `pub` here, because the conformance oracle and
//! the tests reach them the way Downright's `@testable` tests do.
//!
//! The `down` executable itself (`Sources/down/main.swift`) is
//! `src/bin/down/main.rs`.

pub mod agent_bridge;
pub mod agent_watcher;
pub mod doctor;
pub mod markdown_cli;
