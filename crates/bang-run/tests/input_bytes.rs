// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use std::process::{
    Command,
    Output,
    Stdio,
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bang"))
        .args(args)
        .output()
        .expect("spawn bang")
}

#[test]
fn select_ctrl_c_cancels_with_no_output() {
    let output = run(&[
        "select",
        "--option",
        "a",
        "--option",
        "b",
        "--input-bytes",
        "\\x03",
    ]);
    assert_eq!(output.status.code(), Some(130));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn select_ctrl_c_before_submit_still_cancels() {
    let output = run(&[
        "select",
        "--option",
        "a",
        "--option",
        "b",
        "--input-bytes",
        "\\x03\\r",
    ]);
    assert_eq!(output.status.code(), Some(130));
    assert!(output.stdout.is_empty());
}

#[test]
fn select_ctrl_d_ends_input() {
    let output = run(&[
        "select",
        "--option",
        "a",
        "--option",
        "b",
        "--input-bytes",
        "\\x04",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("input ended before \"select\" was answered, so supply the answer")
    );
}

#[test]
fn select_without_a_terminal_names_the_prompt_and_a_remedy() {
    let output = run(&["select", "--option", "a", "--option", "b"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot ask \"select\" because there is no interactive terminal, so pass")
    );
}

#[test]
fn text_ctrl_d_before_typed_input_ends_input() {
    let output = run(&["text", "--input-bytes", "\\x04abc\\r"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn an_empty_option_is_a_usage_error() {
    let output = run(&["select", "--option", "", "--input-bytes", "\\r"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--option cannot be empty"));
}

#[test]
fn the_output_flag_is_global_after_a_subcommand_and_option_has_no_short() {
    let output = run(&[
        "select",
        "--option",
        "a",
        "--input-bytes",
        "\\r",
        "-o",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "\"a\"");
}

#[test]
fn a_broken_pipe_exits_141() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bang"))
        .args(["select", "--option", "a", "--input-bytes", "\\r"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn bang");
    drop(child.stdout.take());
    assert_eq!(child.wait().expect("wait").code(), Some(141));
}
