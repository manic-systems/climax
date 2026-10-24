// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs,
    path::PathBuf,
    process::Command,
};

const FIXTURES: &[&str] = &[
    "fixture-bang-core-only",
    "fixture-bang-only",
    "fixture-bang-terminal-only",
    "fixture-caress-only",
    "fixture-climax-anyhow-only",
    "fixture-climax-complete-only",
    "fixture-climax-derive-only",
    "fixture-climax-interactive-only",
    "fixture-climax-no-default-features",
    "fixture-climax-only",
    "fixture-climax-parse-only",
    "fixture-climax-render-only",
    "fixture-climax-structured-only",
    "fixture-climax-with-components",
    "fixture-pound-color-only",
    "fixture-pound-no-derive",
    "fixture-pound-no-help",
    "fixture-pound-no-std",
    "fixture-pound-only",
    "fixture-pound-unicode-only",
    "fixture-screw-only",
];

/// fixtures whose library carries doctests, which `--all-targets` skips, so
/// they also run `cargo test --doc`
const DOCTEST_FIXTURES: &[&str] = &["fixture-climax-only"];

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

#[test]
fn every_fixture_directory_is_listed() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/compile-fixtures");
    let mut on_disk: Vec<String> = fs::read_dir(directory)
        .expect("read the compile fixtures directory")
        .map(|entry| {
            let name = entry.expect("read a fixture entry").file_name();
            format!("fixture-{}", name.to_string_lossy())
        })
        .collect();
    on_disk.sort();

    assert_eq!(on_disk, FIXTURES);
}
