//! Rust side of the `formats` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/FormatsDump.swift`.

use super::{Failure, Request};

pub fn run(_request: &Request) -> Result<(), Failure> {
    Err(Failure::NotPorted)
}
