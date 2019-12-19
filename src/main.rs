//! Substrate Node Template CLI library.

#![warn(missing_docs)]
#![warn(unused_extern_crates)]

mod chain_spec;
#[macro_use]
mod service;
mod cli;

pub use substrate_cli::{VersionInfo, IntoExit, error};

fn main() {
    let version = VersionInfo {
        name: "Encointer Node",
        commit: env!("VERGEN_SHA_SHORT"),
        version: env!("CARGO_PKG_VERSION"),
        executable_name: "encointer-node",
        author: "encointer.org",
        description: "encointer-node",
        support_url: "encointer.org",
    };

	cli::run(std::env::args(), cli::Exit, version);
}
