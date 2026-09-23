//! Rust side of the `updater` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/UpdaterDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
