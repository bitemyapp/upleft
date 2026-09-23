//! Upleft's port of the non-UI layer of Downright's `DownrightApp` target.
//!
//! One module per Swift file, same names in snake_case, grouped in the same
//! folders as `Sources/DownrightApp`. Swift `internal` declarations are `pub`
//! here, because the conformance oracle and the tests reach them the way
//! Downright's `@testable` tests do. View construction that needs windows
//! arrives with the UI port; everything that computes lives here.
//!
//! `ai::local_ai` and `integrations::app_intents` wrap Swift-only frameworks
//! (FoundationModels, AppIntents); see `PORTING.md` for how they are bridged.

pub mod ai;
pub mod app;
pub mod export;
pub mod integrations;
pub mod panels;
pub mod review;
pub mod security;
pub mod support;
pub mod updater;
pub mod workspace;
