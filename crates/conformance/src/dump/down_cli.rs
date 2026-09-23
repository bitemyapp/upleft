//! Rust side of the `down-cli` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/DownCLIDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
