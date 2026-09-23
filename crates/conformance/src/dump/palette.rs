//! Rust side of the `palette` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/PaletteDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
