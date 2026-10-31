// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use std::process::{
    Command,
    Output,
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bang"))
        .args(args)
        .output()
        .expect("spawn bang")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_plain_action_key_error_uses_generic_wording() {
    let output = run(&[
        "select", "--option", "a", "--option", "b", "--action", "ab:foo",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let message = stderr(&output);
    assert!(
        message.contains("action key 'ab' must be one printable character"),
        "{message}"
    );
    assert!(!message.contains("review action key"), "{message}");
}

#[test]
fn duplicate_plain_action_keys_are_rejected() {
    let output = run(&[
        "select", "--option", "a", "--option", "b", "--action", "x:one", "--action", "x:two",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("duplicate action key 'x'"));
}

#[test]
fn a_review_action_key_reserved_by_built_in_controls_is_rejected() {
    let output = run(&[
        "review-list",
        "--option",
        "a",
        "--action-output",
        "--action",
        "c:custom",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("review action key 'c' is reserved by built-in review controls")
    );
}

#[test]
fn duplicate_review_action_keys_against_the_defaults_are_rejected() {
    let output = run(&[
        "review-list",
        "--option",
        "a",
        "--action-output",
        "--action",
        "g:extra",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("duplicate review action key 'g'"));
}

#[test]
fn control_keys_the_terminal_sends_as_other_keys_are_refused() {
    for (key, sent_as) in [
        ("ctrl-m", "enter"),
        ("ctrl-j", "enter"),
        ("ctrl-i", "tab"),
        ("ctrl-h", "backspace"),
    ] {
        let output = run(&["select", "--option", "a", "--action", &format!("{key}:go")]);
        assert_eq!(output.status.code(), Some(2), "{key}");
        assert!(
            stderr(&output).contains(&format!("is read as {sent_as}, so bind {sent_as} instead")),
            "{key}"
        );
    }
}

#[test]
fn review_action_keys_differing_only_in_case_are_duplicates() {
    let output = run(&[
        "review-list",
        "--option",
        "a",
        "--action-output",
        "--action",
        "z:one",
        "--action",
        "Z:two",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("duplicate review action key 'z'"));
}
