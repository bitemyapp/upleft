//! Rust side of the `spotlight` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/SpotlightDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
