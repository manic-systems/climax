#![cfg(feature = "derive")]

use pound::{
    ErrorKind,
    Parse,
};

#[derive(Debug, Parse)]
enum Leaf {
    Run,
}

#[derive(Debug, Parse)]
struct Root {
    #[pound(long, short = 'm', alias = "selection", global)]
    mode:    Option<String>,
    #[pound(long, global)]
    disable: bool,
    #[pound(short, long, global)]
    help:    bool,
    #[pound(subcommand)]
    command: Middle,
}

#[derive(Debug, Parse)]
enum Middle {
    Inner {
        #[pound(long, short = 'm', alias = "selection", global)]
        mode:    Option<String>,
        #[pound(long, global, negate = "disable", default = "true")]
        enabled: bool,
        #[pound(subcommand)]
        command: Leaf,
    },
}

#[test]
fn nearest_ancestor_owns_a_shared_spelling() {
    for flag in ["--mode=inner", "--selection=inner", "-minner"] {
        let parsed =
            Root::try_parse_from(["--mode=root", "inner", "run", flag, "--disable"]).unwrap();
        let Middle::Inner {
            mode,
            enabled,
            command: Leaf::Run,
        } = parsed.command;
        assert_eq!(parsed.mode.as_deref(), Some("root"), "{flag}");
        assert_eq!(mode.as_deref(), Some("inner"), "{flag}");
        assert!(!parsed.disable, "{flag}");
        assert!(!enabled, "{flag}");
    }
    assert!(Root::try_parse_from(["inner", "run", "-h"]).unwrap().help);
    let error = Root::try_parse_from(["inner", "run", "--disable=true"]).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedValue("--disable".into()));
}
