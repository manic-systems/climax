// SPDX-License-Identifier: EUPL-1.2

//! environment-variable fallback: the command line wins, then `env`, then
//! `default`. env is resolved by the typed readers, so it never enters the
//! borrowed `Matches`.

use pound::{
    Error,
    Parse,
};

#[derive(Parse)]
#[pound(name = "app")]
struct Cli {
    #[pound(long, env = "POUND_TOKEN")]
    token: Option<String>,
    #[pound(long, env = "POUND_LEVEL", default = "info")]
    level: String,
}

#[derive(Parse)]
#[pound(name = "req")]
struct Req {
    #[pound(long, env = "POUND_REQ")]
    req: String,
}

#[derive(Parse, Debug)]
#[pound(name = "bounded")]
struct Bounded {
    #[pound(long, env = "POUND_BOUNDED", max_len = "3")]
    value: String,
}

fn set(k: &str, v: &str) {
    unsafe { std::env::set_var(k, v) };
}
fn clear(k: &str) {
    unsafe { std::env::remove_var(k) };
}
const fn no_args() -> Vec<&'static str> {
    Vec::new()
}

// one test fn, so the process-global environment is only touched serially.
#[test]
fn env_fallback_precedence() {
    clear("POUND_TOKEN");
    clear("POUND_LEVEL");
    clear("POUND_REQ");
    clear("POUND_BOUNDED");

    // nothing set: the optional is None, the bare field uses its default
    let c = Cli::try_parse_from(no_args()).unwrap();
    assert_eq!(c.token, None);
    assert_eq!(c.level, "info");

    // env fills a value the command line omitted, and beats the default
    set("POUND_TOKEN", "from-env");
    set("POUND_LEVEL", "debug");
    let c = Cli::try_parse_from(no_args()).unwrap();
    assert_eq!(c.token.as_deref(), Some("from-env"));
    assert_eq!(c.level, "debug");

    // the command line beats env
    let c = Cli::try_parse_from(["--token", "cli", "--level", "trace"]).unwrap();
    assert_eq!(c.token.as_deref(), Some("cli"));
    assert_eq!(c.level, "trace");

    // env satisfies a required field, and its absence is still an error
    assert!(Req::try_parse_from(no_args()).is_err());
    set("POUND_REQ", "ok");
    assert_eq!(Req::try_parse_from(no_args()).unwrap().req, "ok");

    // env fallback still goes through field validation
    set("POUND_BOUNDED", "toolong");
    match Bounded::try_parse_from(no_args()) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "toolong");
            assert_eq!(msg, "must be at most 3 chars");
        },
        other => panic!("expected env validation error, got {other:?}"),
    }
    assert_eq!(
        Bounded::try_parse_from(["--value", "cli"]).unwrap().value,
        "cli"
    );

    clear("POUND_TOKEN");
    clear("POUND_LEVEL");
    clear("POUND_REQ");
    clear("POUND_BOUNDED");
}
