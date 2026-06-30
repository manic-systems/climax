// SPDX-License-Identifier: EUPL-1.2

//! end-to-end exercise of the public `Parse` surface by hand-writing exactly
//! what the derive will emit. if this stays pleasant by hand the generated
//! version will be too, and these double as the spec the macro must satisfy.

use pound::{
    ArgSpec,
    CommandSpec,
    Error,
    FromArg,
    Kind,
    Matches,
    Parse,
    SubSpec,
    ValueError,
};

fn argv<'a>(a: &[&'a str]) -> Vec<&'a str> {
    a.to_vec()
}

// a flat command: flags, a repeatable option, and a trailing exec

struct Sandbox {
    sockets: bool,
    env:     Vec<String>,
    exec:    Vec<String>,
}

static SANDBOX_ARGS: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag)
        .long("sockets")
        .short('s')
        .help("allow unix sockets"),
    ArgSpec::new(Kind::Opt)
        .long("env")
        .short('e')
        .multi()
        .value_name("k=v")
        .help("set env var"),
    ArgSpec::new(Kind::Trailing)
        .value_name("command")
        .help("program to run"),
];
static SANDBOX_SPEC: CommandSpec = CommandSpec::new("sandbox")
    .version("0.1.0")
    .about("simple sandboxer")
    .args(SANDBOX_ARGS);

impl Parse for Sandbox {
    const SPEC: &'static CommandSpec = &SANDBOX_SPEC;

    fn from_matches(spec: &'static CommandSpec, m: &Matches) -> Result<Self, Error> {
        Ok(Self {
            sockets: m.flag(0),
            env:     m.many::<String>(spec, 1)?,
            exec:    m.many::<String>(spec, 2)?,
        })
    }
}

#[test]
fn sandbox_flat() {
    let y = Sandbox::parse_from(argv(&[
        "-s", "--env", "A=1", "-e", "B=2", "--", "ls", "-la",
    ]));
    assert!(y.sockets);
    assert_eq!(y.env, ["A=1", "B=2"]);
    assert_eq!(y.exec, ["ls", "-la"]);
}

// a subcommand tree

#[derive(Debug, PartialEq, Eq)]
enum Pkg {
    Init {
        force: bool,
    },
    Add {
        name:  String,
        url:   String,
        force: bool,
    },
}

static INIT_ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Flag)
    .long("force")
    .short('f')
    .help("overwrite config")];
static INIT_SPEC: CommandSpec = CommandSpec::new("init")
    .about("initialise a project")
    .args(INIT_ARGS);

static ADD_ARGS: &[ArgSpec] = &[
    ArgSpec::new(Kind::Positional)
        .value_name("name")
        .required()
        .help("pin name"),
    ArgSpec::new(Kind::Positional)
        .value_name("url")
        .required()
        .help("source url"),
    ArgSpec::new(Kind::Flag)
        .long("force")
        .short('f')
        .help("overwrite existing"),
];
static ADD_SPEC: CommandSpec = CommandSpec::new("add").about("add a pin").args(ADD_ARGS);

static PKG_SUBS: &[SubSpec] = &[
    SubSpec::new("init", &INIT_SPEC).about("initialise a project"),
    SubSpec::new("add", &ADD_SPEC).about("add a pin"),
];
static PKG_SPEC: CommandSpec = CommandSpec::new("pkg")
    .version("1.0.0")
    .about("a small package manager")
    .subs(PKG_SUBS);

impl Parse for Pkg {
    const SPEC: &'static CommandSpec = &PKG_SPEC;

    fn from_matches(spec: &'static CommandSpec, m: &Matches) -> Result<Self, Error> {
        match m.sub() {
            Some((0, sm)) => Ok(Self::Init { force: sm.flag(0) }),
            Some((1, sm)) => {
                let s = spec.subs[1].spec;
                Ok(Self::Add {
                    name:  sm.required::<String>(s, 0)?,
                    url:   sm.required::<String>(s, 1)?,
                    force: sm.flag(2),
                })
            },
            _ => Err(Error::MissingSubcommand),
        }
    }
}

#[test]
fn pkg_subcommands() {
    assert_eq!(Pkg::parse_from(argv(&["init", "--force"])), Pkg::Init {
        force: true,
    });
    assert_eq!(
        Pkg::parse_from(argv(&["add", "serde", "https://x", "-f"])),
        Pkg::Add {
            name:  "serde".into(),
            url:   "https://x".into(),
            force: true,
        }
    );
}

#[test]
fn pkg_help_screen() {
    let Err(Error::Help(text)) = Pkg::try_parse_from(argv(&["--help"])) else {
        panic!("expected help");
    };
    assert!(text.contains("Usage: pkg"));
    // the sectioned screen only exists with the `help` feature on
    #[cfg(feature = "help")]
    {
        assert!(text.contains("Commands:"));
        assert!(text.contains("add"));
        assert!(text.contains("-h, --help"));
    }
    // an empty invocation also yields help
    assert!(matches!(
        Pkg::try_parse_from(argv(&[])),
        Err(Error::Help(_))
    ));
}

// a custom value type via FromArg with a closed choice set

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Fast,
    Slow,
}

impl FromArg for Mode {
    fn from_arg(s: &str) -> Result<Self, ValueError> {
        match s {
            "fast" => Ok(Self::Fast),
            "slow" => Ok(Self::Slow),
            other => Err(ValueError::new(other, "expected fast or slow")),
        }
    }

    fn possible_values() -> Option<&'static [&'static str]> {
        Some(&["fast", "slow"])
    }
}

struct Run {
    mode: Mode,
}

static RUN_ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt)
    .long("mode")
    .required()
    .value_name("mode")
    .possible(&["fast", "slow"])];
static RUN_SPEC: CommandSpec = CommandSpec::new("run").args(RUN_ARGS);

impl Parse for Run {
    const SPEC: &'static CommandSpec = &RUN_SPEC;

    fn from_matches(spec: &'static CommandSpec, m: &Matches) -> Result<Self, Error> {
        Ok(Self {
            mode: m.required::<Mode>(spec, 0)?,
        })
    }
}

#[test]
fn custom_from_arg() {
    assert_eq!(Run::parse_from(argv(&["--mode", "fast"])).mode, Mode::Fast);
    match Run::try_parse_from(argv(&["--mode", "warp"])) {
        Err(Error::Value { value, .. }) => assert_eq!(value, "warp"),
        _ => panic!("expected a value error"),
    }
}
