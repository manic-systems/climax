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

/// Dev-dependency edges that must stay path-only. Cargo drops a path-only
/// dev-dependency when packaging but keeps one that carries a version, which
/// would publish an edge to the unpublished screw-pty or a cycle between
/// pound-derive and pound.
const PATH_ONLY_DEV_EDGES: &[(&str, &str)] = &[("pound-derive", "pound"), ("screw", "screw-pty")];

/// Crates released in lockstep with `climax`, whose workspace entries must pin
/// one exact version. `pound` and `caress` release on their own and keep
/// their ordinary caret requirements.
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

/// pound's derive output calls pound's hidden internals, so the impl version
/// must follow pound's own exact pin and not climax's.
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
