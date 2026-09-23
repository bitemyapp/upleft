//! Rust side of the `html-export` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/HTMLExportDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
