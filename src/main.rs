//! Substrate Node Template CLI library.
#![warn(missing_docs)]

mod chain_spec;
#[macro_use]
mod service;
mod cli;
mod command;

pub use sc_cli::{VersionInfo, error};

fn main() -> Result<(), error::Error> {
    let version = VersionInfo {
        name: "Substrate Node",
        commit: env!("VERGEN_SHA_SHORT"),
        version: env!("CARGO_PKG_VERSION"),
        executable_name: "substratee-node",
        author: "Supercomputing Systems AG",
        description: "substratee-node",
        support_url: "support.anonymous.an",
        copyright_start_year: 2017,
    };

    command::run(version)
}
