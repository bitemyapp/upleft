//! Rust side of the `local-ai` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/LocalAIDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
