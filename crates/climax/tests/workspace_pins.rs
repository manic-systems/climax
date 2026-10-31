// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

use toml::{
    Table,
    Value,
};

/// dev-dependency edges that must stay path-only, because cargo keeps a
/// versioned one when packaging and would publish a pound-derive cycle
const PATH_ONLY_DEV_EDGES: &[(&str, &str)] = &[("pound-derive", "pound")];

/// crates released in lockstep with `climax`, pinned to one exact version,
/// while `pound` and `caress` release on their own and keep caret requirements
const CLIMAX_FAMILY: &[&str] = &[
    "bang",
    "bang-core",
    "bang-run",
    "bang-terminal",
    "climax",
    "climax-derive",
    "screw",
    "screw-pty",
];

/// pound's derive output calls its hidden internals, so the impl version
/// follows pound's pin and not climax's
const CARET_EDGES: &[(&str, &str)] = &[("climax-derive", "pound-derive-impl")];

const DEPENDENCY_TABLES: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("climax crate lives under the workspace root")
        .to_owned()
}

fn read(path: &Path) -> Table {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        .parse()
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn workspace_crates(root: &Table) -> Vec<(String, PathBuf)> {
    root["workspace"]["members"]
        .as_array()
        .expect("workspace members")
        .iter()
        .filter_map(Value::as_str)
        .filter(|member| member.starts_with("crates/"))
        .map(|member| {
            let manifest = workspace_root().join(member).join("Cargo.toml");
            let name = read(&manifest)["package"]["name"]
                .as_str()
                .expect("package name")
                .to_owned();
            (name, manifest)
        })
        .collect()
}

#[test]
fn workspace_dependencies_on_workspace_crates_are_exact_pins() {
    let root = read(&workspace_root().join("Cargo.toml"));
    let crates = workspace_crates(&root);
    let shared = root["workspace"]["dependencies"]
        .as_table()
        .expect("workspace dependencies");

    let mut failures = Vec::new();
    for (name, _) in crates
        .iter()
        .filter(|(name, _)| CLIMAX_FAMILY.contains(&name.as_str()))
    {
        let Some(entry) = shared.get(name) else {
            continue;
        };
        let version = entry.get("version").and_then(Value::as_str);
        if !version.is_some_and(|text| text.starts_with('=')) {
            failures.push(format!(
                "[workspace.dependencies] {name} lacks an exact `=` version, found {version:?}"
            ));
        }
    }

    for (package, manifest) in &crates {
        let table = read(manifest);
        let mut tables: Vec<&Table> = DEPENDENCY_TABLES
            .iter()
            .filter_map(|key| table.get(*key).and_then(Value::as_table))
            .collect();
        if let Some(targets) = table.get("target").and_then(Value::as_table) {
            for target in targets.values().filter_map(Value::as_table) {
                tables.extend(
                    DEPENDENCY_TABLES
                        .iter()
                        .filter_map(|key| target.get(*key).and_then(Value::as_table)),
                );
            }
        }
        for dependencies in tables {
            for (name, _) in &crates {
                let Some(entry) = dependencies.get(name) else {
                    continue;
                };
                if PATH_ONLY_DEV_EDGES.contains(&(package.as_str(), name.as_str())) {
                    let path_only = entry.get("path").is_some()
                        && entry.get("version").is_none()
                        && entry.get("workspace").is_none();
                    if !path_only {
                        failures.push(format!(
                            "{package} must depend on {name} by path alone so packaging drops it"
                        ));
                    }
                    continue;
                }
                if CARET_EDGES.contains(&(package.as_str(), name.as_str())) {
                    let caret = entry
                        .get("version")
                        .and_then(Value::as_str)
                        .is_some_and(|version| !version.starts_with('='))
                        && entry.get("workspace").is_none();
                    if !caret {
                        failures.push(format!("{package} must require {name} at a caret range"));
                    }
                    continue;
                }
                let inherited = entry.get("workspace").and_then(Value::as_bool) == Some(true);
                if !inherited {
                    failures.push(format!(
                        "{package} depends on {name} without `workspace = true`"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "climax family dependencies are not exactly pinned:\n{}",
        failures.join("\n"),
    );
}

#[test]
fn tests_that_read_the_workspace_root_are_left_out_of_packages() {
    let root = read(&workspace_root().join("Cargo.toml"));
    let mut failures = Vec::new();
    for (name, manifest) in workspace_crates(&root) {
        let excluded: Vec<String> = read(&manifest)["package"]
            .get("exclude")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let Ok(entries) = fs::read_dir(manifest.with_file_name("tests")) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("read a test entry").path();
            let source = fs::read_to_string(&path).unwrap_or_default();
            let relative = format!(
                "tests/{}",
                path.file_name().expect("test file name").to_string_lossy()
            );
            let reads_root = ["/../../", ".nth(2)", "crates.parent()"]
                .iter()
                .any(|marker| source.contains(marker));
            if reads_root && !excluded.contains(&relative) {
                failures.push(format!(
                    "{name} packages {relative}, which needs the workspace"
                ));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_workspace_rust_version_covers_pound() {
    let root = read(&workspace_root().join("Cargo.toml"));
    let declared = root["workspace"]["package"]["rust-version"]
        .as_str()
        .expect("workspace rust-version");
    let minor: u32 = declared
        .split('.')
        .nth(1)
        .and_then(|part| part.parse().ok())
        .expect("rust-version has a minor component");
    assert!(minor >= 89, "rust-version {declared} is below 1.89");
}

#[test]
fn shared_external_dependencies_are_inherited_from_the_workspace() {
    let root = read(&workspace_root().join("Cargo.toml"));
    let shared = root["workspace"]["dependencies"]
        .as_table()
        .expect("workspace dependencies");
    let crates = workspace_crates(&root);

    let mut failures = Vec::new();
    for (package, manifest) in &crates {
        let table = read(manifest);
        for key in DEPENDENCY_TABLES {
            let Some(dependencies) = table.get(*key).and_then(Value::as_table) else {
                continue;
            };
            for (name, entry) in dependencies {
                if !shared.contains_key(name) || crates.iter().any(|(member, _)| member == name) {
                    continue;
                }
                if entry.get("workspace").and_then(Value::as_bool) != Some(true) {
                    failures.push(format!(
                        "{package} declares {name} instead of inheriting it"
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
