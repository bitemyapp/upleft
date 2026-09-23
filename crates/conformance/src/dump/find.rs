//! Rust side of the `find` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/FindDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
