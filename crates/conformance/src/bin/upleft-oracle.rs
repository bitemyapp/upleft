//! `upleft-oracle` — the Rust side of the conformance harness. It takes the
//! arguments `downright-oracle` takes and writes the same formats:
//!
//!   upleft-oracle <command> <file.md> <out> [flags…]
//!
//! A command whose layer has not been ported yet exits with status 3, which
//! `conform` reports as "not ported".

use std::process::ExitCode;

use upleft_conformance::dump::{self, Request};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let request = match Request::parse(&arguments) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("upleft-oracle: {message}");
            eprintln!("usage: upleft-oracle <command> <file.md> <out> [--mode M] [--theme NAME] [--dark] [--width W] [--height H] [--layout out.json]");
            return ExitCode::from(64);
        }
    };
    match dump::run(&request) {
        Ok(()) => ExitCode::SUCCESS,
        Err(dump::Failure::NotPorted) => {
            eprintln!("upleft-oracle: `{}` is not ported yet", request.command);
            ExitCode::from(3)
        }
        Err(dump::Failure::Error(message)) => {
            eprintln!("upleft-oracle: {message}");
            ExitCode::from(1)
        }
    }
}
