// SPDX-License-Identifier: EUPL-1.2

use std::{
    path::PathBuf,
    process::Command,
};

const FIXTURES: &[&str] = &[];

/// each fixture gets its own `cargo test -p`, since a `--workspace` run would
/// unify features and hide a fixture that only compiles because a sibling
/// enabled a feature it needs
#[test]
fn documented_dependency_stories_compile_in_isolation() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("climax crate lives under the workspace root")
        .join("Cargo.toml");
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));

    let failures: Vec<String> = FIXTURES
        .iter()
        .filter_map(|&fixture| {
            let output = Command::new(env!("CARGO"))
                .args([
                    "test",
                    "--offline",
                    "--locked",
                    "-p",
                    fixture,
                    "--all-targets",
                    "--manifest-path",
                ])
                .arg(&manifest)
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .expect("run cargo for a dependency fixture");

            (!output.status.success()).then(|| {
                format!(
                    "{fixture}:\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                )
            })
        })
        .collect();

    assert!(
        failures.is_empty(),
        "dependency fixtures failed in isolation:\n{}",
        failures.join("\n"),
    );
}
