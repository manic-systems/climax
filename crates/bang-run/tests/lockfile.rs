// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use std::fs;

#[test]
fn the_lockfile_holds_one_toml_tree() {
    let lock = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
        .expect("read the workspace lockfile");
    let tomls = lock
        .lines()
        .filter(|line| *line == "name = \"toml\"")
        .count();

    assert_eq!(tomls, 1);
}
