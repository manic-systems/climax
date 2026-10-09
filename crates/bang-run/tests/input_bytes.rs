// SPDX-License-Identifier: EUPL-1.2

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bang"))
        .args(args)
        .output()
        .expect("spawn bang")
}

#[test]
fn select_ctrl_c_cancels_with_no_output() {
    let output = run(&["select", "-o", "a", "-o", "b", "--input-bytes", "\\x03"]);
    assert_eq!(output.status.code(), Some(130));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn select_ctrl_c_before_submit_still_cancels() {
    let output = run(&["select", "-o", "a", "-o", "b", "--input-bytes", "\\x03\\r"]);
    assert_eq!(output.status.code(), Some(130));
    assert!(output.stdout.is_empty());
}

#[test]
fn select_ctrl_d_ends_input() {
    let output = run(&["select", "-o", "a", "-o", "b", "--input-bytes", "\\x04"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("input ended before the prompt was submitted"));
}

#[test]
fn text_ctrl_d_before_typed_input_ends_input() {
    let output = run(&["text", "--input-bytes", "\\x04abc\\r"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}
