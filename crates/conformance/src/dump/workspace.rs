//! Rust side of the `workspace` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/WorkspaceDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
