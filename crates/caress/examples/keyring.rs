// SPDX-License-Identifier: EUPL-1.2

//! completions for a secrets tool, run `COMPLETE=bash cargo run --example
//! keyring` for the script, or `COMPLETE=fish cargo run --example keyring -- --
//! open ''` for the candidates after `keyring open`.

use std::path::PathBuf;

use pound::{
    Parse,
    ValueEnum,
    complete::{
        Candidate,
        Partial,
    },
};

/// keep secrets in age-encrypted files
#[derive(Parse)]
#[pound(name = "keyring", version = "0.1.0")]
#[allow(dead_code, reason = "demo only completes")]
struct Keyring {
    /// the secrets file
    #[pound(short, long, global, default = "secrets.nix")]
    file: PathBuf,
    #[pound(subcommand)]
    cmd:  Cmd,
}

#[derive(Parse)]
#[allow(dead_code, reason = "demo only completes")]
enum Cmd {
    /// decrypt a secret to stdout
    Open {
        /// the secret to decrypt
        #[pound(complete = secrets)]
        secret: String,
        /// how to print it
        #[pound(long, default = "raw")]
        format: Format,
    },
    /// re-encrypt every secret for the current recipients
    Rekey,
}

#[derive(ValueEnum)]
enum Format {
    Raw,
    Json,
}

/// a real tool would read the file `--file` names, which the line already
/// carries
fn secrets(partial: &Partial<'_, '_>, _typed: &str) -> Vec<Candidate> {
    let file = partial
        .arg("--file")
        .and_then(|file| file.last())
        .unwrap_or("secrets.nix");
    ["db-password", "api-token"]
        .into_iter()
        .map(|name| Candidate::new(name).help(format!("from {file}")))
        .collect()
}

fn main() {
    caress::handle(Keyring::SPEC);
    let _keyring = Keyring::parse();
}
