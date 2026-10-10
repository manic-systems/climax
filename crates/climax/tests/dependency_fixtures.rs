// SPDX-License-Identifier: EUPL-1.2

use std::{path::PathBuf, process::Command};

const FIXTURES: &[&str] = &[
    "fixture-bang-only",
    "fixture-climax-derive-only",
    "fixture-climax-interactive-only",
    "fixture-climax-no-default-features",
    "fixture-climax-only",
    "fixture-climax-parse-only",
    "fixture-climax-render-only",
    "fixture-climax-structured-only",
    "fixture-climax-with-components",
    "fixture-pound-no-derive",
    "fixture-pound-no-help",
    "fixture-pound-no-std",
    "fixture-pound-only",
    "fixture-screw-only",
];

/// Fixtures whose library carries doctests. `--all-targets` leaves doctests
/// out, so these also run `cargo test --doc` to prove the README examples with
/// `climax` as the only dependency.
const DOCTEST_FIXTURES: &[&str] = &["fixture-climax-only"];

/// Each fixture is built and tested in its own `cargo test -p`, not
/// `--workspace`, so Cargo resolves its features from only that fixture's
/// dependency graph. A combined `--workspace` run would unify features across
/// every fixture and hide a fixture that only compiles because a sibling enabled
/// a feature it needs.
#[test]
fn documented_dependency_stories_compile_in_isolation() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("climax crate lives under the workspace root")
        .join("Cargo.toml");
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));

    let run = |fixture: &str, target_flag: &str| {
        let output = Command::new(env!("CARGO"))
            .args(["test", "--offline", "--locked", "-p", fixture, target_flag])
            .arg("--manifest-path")
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .expect("run cargo for a dependency fixture");

        (!output.status.success()).then(|| {
            format!(
                "{fixture} {target_flag}:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            )
        })
    };

    let failures: Vec<String> = FIXTURES
        .iter()
        .flat_map(|&fixture| {
            let doctests = DOCTEST_FIXTURES.contains(&fixture).then_some("--doc");
            [Some("--all-targets"), doctests]
                .into_iter()
                .flatten()
                .filter_map(move |flag| run(fixture, flag))
        })
        .collect();

    assert!(
        failures.is_empty(),
        "dependency fixtures failed in isolation:\n{}",
        failures.join("\n"),
    );
}
