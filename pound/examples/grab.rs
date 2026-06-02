// SPDX-License-Identifier: EUPL-1.2

//! a tiny demo command, run `cargo run --example grab -- --help`.

use pound::Parse;

/// fetch urls to disk
#[derive(Parse, Debug)]
#[pound(name = "grab", version = "0.1.0")]
#[allow(dead_code, reason = "demo just prints the parsed struct")]
struct Grab {
    /// urls to fetch
    url: Vec<String>,
    /// write downloads under this directory
    #[pound(short, long)]
    output: Option<String>,
    /// overwrite existing files
    #[pound(short, long)]
    force: bool,
    /// increase verbosity, repeatable
    #[pound(short, long, count)]
    verbose: u8,
    /// number of parallel jobs
    #[pound(short, long, default = "4")]
    jobs: u32,
}

fn main() {
    let grab = Grab::parse();
    println!("{grab:?}");
}
