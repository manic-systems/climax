// SPDX-License-Identifier: EUPL-1.2

#![cfg(all(feature = "derive", feature = "help"))]

use pound::{
    ErrorKind,
    Parse,
    Term,
};

#[derive(Debug, Parse)]
#[pound(name = "fetch")]
struct Fetch {
    /// number of downloads that may run at the same time
    #[pound(long)]
    jobs: Option<u32>,
}

fn without_styles(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            assert_eq!(chars.next(), Some('['), "escape without a CSI");
            assert!(chars.by_ref().any(|end| end == 'm'), "unterminated escape");
        } else {
            out.push(ch);
        }
    }
    out
}

#[test]
fn coloring_does_not_change_help_layout_or_plain_display() {
    let err = Fetch::try_parse_from(["--help"]).unwrap_err();
    let ErrorKind::Help(help) = err.kind else {
        panic!("expected help");
    };
    let plain = help.render(Term::new(Some(40), false));
    let colored = help.render(Term::new(Some(40), true));
    assert_eq!(without_styles(&colored), plain);
    assert_eq!(colored.contains('\x1b'), cfg!(feature = "color"));
    assert!(!help.to_string().contains('\x1b'));
    assert!(plain.contains("--jobs"));
}

#[test]
fn coloring_preserves_diagnostics_and_suggestions() {
    let err = Fetch::try_parse_from(["--job", "2"]).unwrap_err();
    assert_eq!(err.kind.closest(), Some("--jobs"));
    let plain = err.render(Term::plain());
    let colored = err.render(Term::new(None, true));
    assert_eq!(without_styles(&colored), plain);
    assert_eq!(colored.contains('\x1b'), cfg!(feature = "color"));
    assert!(!err.to_string().contains('\x1b'));
    assert!(plain.contains("did you mean '--jobs'"));
}
