#![allow(
    dead_code,
    reason = "demo fields expose metadata without performing downloads"
)]

use std::{
    fmt::Write as _,
    path::Path,
};

use pound::{
    ArgSpec,
    CommandSpec,
    Kind,
    Parse,
    ValueEnum,
};

#[derive(Debug, Parse)]
/// inspect a download cli through pound metadata
#[pound(name = "command_docs")]
struct Demo {
    #[pound(flatten)]
    shared: Shared,
}

#[derive(Debug, Parse)]
struct Shared {
    #[pound(flatten)]
    output:   Output,
    #[pound(short, long, count, global, help = "Increase diagnostic detail")]
    verbose:  u8,
    #[pound(long, global, hidden)]
    trace:    bool,
    #[pound(flatten)]
    commands: CommandOptions,
}

#[derive(Debug, Parse)]
struct CommandOptions {
    #[pound(subcommand)]
    command: Action,
}

#[derive(Debug, Parse)]
struct Output {
    #[pound(
        short,
        long,
        alias = "output-format",
        global,
        default = "text",
        help = "Choose the output encoding"
    )]
    format: Format,
}

#[derive(Debug, ValueEnum)]
enum Format {
    Text,
    Json,
    Toml,
}

#[derive(Debug, ValueEnum)]
enum Shell {
    Bash,
    Fish,
}

#[derive(Debug, Parse)]
enum Action {
    /// describe a command using custom help
    Help { path: Vec<String> },
    /// generate a shell completion script
    Completions { shell: Shell },
    #[pound(flatten)]
    Download(DownloadAction),
    #[pound(name = "__complete", hidden)]
    Complete { words: Vec<String> },
}

#[derive(Debug, Parse)]
enum DownloadAction {
    /// print the parsed download request
    #[pound(alias = "get")]
    Fetch {
        #[pound(value_name = "URL", help = "Address to download")]
        url:     String,
        #[pound(flatten)]
        network: Network,
    },
    /// inspect cached downloads
    Cache {
        #[pound(subcommand)]
        command: CacheAction,
    },
}

#[derive(Debug, Parse)]
struct Network {
    #[pound(
        short,
        long,
        default = "3",
        env = "POUND_DEMO_RETRIES",
        help = "Retry failed downloads"
    )]
    retries: u8,
}

#[derive(Debug, Parse)]
enum CacheAction {
    /// list cached downloads
    #[pound(alias = "ls")]
    List,
    /// remove cached downloads
    Clean {
        #[pound(short, long, help = "Remove every cached download")]
        all: bool,
    },
    #[pound(hidden)]
    Doctor,
}

struct Context<'a> {
    spec:    &'a CommandSpec,
    globals: Vec<&'a ArgSpec>,
    path:    String,
}

impl<'a> Context<'a> {
    fn new(spec: &'a CommandSpec) -> Self {
        Self {
            spec,
            globals: Vec::new(),
            path: spec.name.to_owned(),
        }
    }

    fn descend(&mut self, name: &str) -> bool {
        let Some(sub) = self.spec.find_sub(name) else {
            return false;
        };
        self.globals
            .extend(self.spec.arguments().filter(|arg| arg.global));
        self.path.push(' ');
        self.path.push_str(sub.name);
        self.spec = sub.spec;
        true
    }

    fn arguments(&self) -> impl Iterator<Item = &'a ArgSpec> + '_ {
        self.spec.arguments().chain(self.globals.iter().copied())
    }

    fn long(&self, name: &str) -> Option<&'a ArgSpec> {
        self.spec
            .arguments()
            .chain(self.globals.iter().rev().copied())
            .find(|arg| {
                arg.long == Some(name) || arg.aliases.contains(&name) || arg.negate == Some(name)
            })
    }

    fn short(&self, name: char) -> Option<&'a ArgSpec> {
        self.spec
            .arguments()
            .chain(self.globals.iter().rev().copied())
            .find(|arg| arg.short == Some(name))
    }

    fn builtins(&self) -> Vec<(&'static str, &'static str)> {
        let mut flags = Vec::new();
        if self.short('h').is_none() {
            flags.push(("-h", "Show builtin help"));
        }
        if self.long("help").is_none() {
            flags.push(("--help", "Show builtin help"));
        }
        if self.spec.has_version_info() {
            if self.short('V').is_none() {
                flags.push(("-V", "Show version information"));
            }
            if self.long("version").is_none() {
                flags.push(("--version", "Show version information"));
            }
        }
        flags
    }
}

fn custom_help(spec: &CommandSpec, path: &[String]) -> Result<String, String> {
    let mut context = Context::new(spec);
    for name in path {
        if !context.descend(name) {
            return Err(format!("Unknown command {name}"));
        }
    }
    let mut text = format!(
        "{}\n{}\n\nUsage  {}",
        context.path, context.spec.about, context.path
    );
    if context
        .arguments()
        .any(|arg| !arg.hidden && !arg.is_positional())
        || !context.builtins().is_empty()
    {
        text.push_str(" [OPTIONS]");
    }
    for arg in context
        .spec
        .arguments()
        .filter(|arg| arg.is_positional() && !arg.hidden)
    {
        let name = metavar(arg);
        let dots = if arg.multi || arg.kind == Kind::Trailing {
            "..."
        } else {
            ""
        };
        if arg.required {
            let _ = write!(text, " <{name}>{dots}");
        } else {
            let _ = write!(text, " [{name}]{dots}");
        }
    }
    if context.spec.has_subs() {
        text.push_str(if context.spec.subcommand_optional() {
            " [COMMAND]"
        } else {
            " <COMMAND>"
        });
    }
    text.push('\n');
    if context.spec.has_version_info() {
        let _ = write!(text, "\nVersion  {}", context.spec.version);
        if let Some(hash) = context.spec.hash {
            let _ = write!(text, " ({hash})");
        }
        text.push('\n');
    }
    append_argument_help(&mut text, &context);
    let subs = context
        .spec
        .subcommands()
        .filter(|sub| !sub.hidden)
        .collect::<Vec<_>>();
    if !subs.is_empty() {
        text.push_str("\nCommands\n");
        for sub in subs {
            let aliases = if sub.aliases.is_empty() {
                String::new()
            } else {
                format!(" ({})", sub.aliases.join(", "))
            };
            let _ = writeln!(text, "  {}{aliases}  {}", sub.name, sub.about);
        }
    }
    Ok(text)
}

fn append_argument_help(text: &mut String, context: &Context<'_>) {
    for arg in context.arguments().filter(|arg| !arg.hidden) {
        let mut names = Vec::new();
        if let Some(short) = arg.short {
            names.push(format!("-{short}"));
        }
        names.extend(
            arg.long
                .into_iter()
                .chain(arg.aliases.iter().copied())
                .chain(arg.negate)
                .map(|name| format!("--{name}")),
        );
        let mut label = names.join(", ");
        if arg.is_positional() {
            label = metavar(arg);
        } else if arg.takes_value() {
            let _ = write!(label, " <{}>", metavar(arg));
        }
        let _ = write!(text, "\n  {label}\n      {}", arg.help);
        if let Some(values) = arg.possible {
            let _ = write!(text, " [choices {}]", values.join(", "));
        }
        if let Some(value) = arg.default {
            let _ = write!(text, " [default {value}]");
        }
        if let Some(env) = arg.env {
            let _ = write!(text, " [env {env}]");
        }
        if arg.required {
            text.push_str(" [required]");
        }
        if arg.multi {
            text.push_str(" [repeatable]");
        }
        if arg.global {
            text.push_str(" [global]");
        }
        text.push('\n');
    }
    for (name, description) in context.builtins() {
        let _ = writeln!(text, "\n  {name}\n      {description}");
    }
}

fn metavar(arg: &ArgSpec) -> String {
    if arg.value_name.is_empty() {
        arg.long.unwrap_or("value").to_uppercase()
    } else {
        arg.value_name.to_uppercase()
    }
}

fn values(arg: &ArgSpec, prefix: &str, spelling: &str) -> Vec<String> {
    if arg.hidden {
        return Vec::new();
    }
    arg.possible
        .unwrap_or_default()
        .iter()
        .filter(|value| value.starts_with(prefix))
        .map(|value| format!("{spelling}{value}"))
        .collect()
}

fn complete(spec: &CommandSpec, words: &[&str]) -> Vec<String> {
    let (&prefix, consumed) = words.split_last().unwrap_or((&"", &[]));
    let mut context = Context::new(spec);
    let mut pending = None;
    let mut positional = 0;
    let mut options = true;
    for &word in consumed {
        if pending.take().is_some() {
            continue;
        }
        if options && word == "--" {
            options = false;
            continue;
        }
        if options && let Some(long) = word.strip_prefix("--") {
            let (name, inline) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            let Some(arg) = context.long(name) else {
                return Vec::new();
            };
            if arg.takes_value() && arg.default_missing.is_none() && !inline {
                pending = Some(arg);
            }
            continue;
        }
        if options
            && let Some(shorts) = word.strip_prefix('-').filter(|s| !s.is_empty())
            && context.short(shorts.chars().next().unwrap()).is_some()
        {
            for (offset, short) in shorts.char_indices() {
                let Some(arg) = context.short(short) else {
                    return Vec::new();
                };
                if arg.takes_value() {
                    if offset + short.len_utf8() == shorts.len() && arg.default_missing.is_none() {
                        pending = Some(arg);
                    }
                    break;
                }
            }
            continue;
        }
        if options
            && context.spec.has_subs()
            && command_available(context.spec, positional)
            && context.descend(word)
        {
            positional = 0;
            continue;
        }
        let Some(arg) = context
            .spec
            .arguments()
            .filter(|arg| arg.is_positional())
            .nth(positional)
        else {
            return Vec::new();
        };
        if !arg.multi && arg.kind != Kind::Trailing {
            positional += 1;
        }
    }
    if let Some(arg) = pending {
        return values(arg, prefix, "");
    }
    if options
        && let Some((name, value)) = prefix.strip_prefix("--").and_then(|s| s.split_once('='))
    {
        return context
            .long(name)
            .filter(|arg| arg.takes_value())
            .map_or_else(Vec::new, |arg| values(arg, value, &format!("--{name}=")));
    }
    if options && let Some(shorts) = prefix.strip_prefix('-').filter(|s| !s.starts_with('-')) {
        for (offset, short) in shorts.char_indices() {
            let Some(arg) = context.short(short) else {
                break;
            };
            if arg.takes_value() {
                let start = offset + short.len_utf8();
                return values(arg, &shorts[start..], &prefix[..=start]);
            }
        }
    }
    completion_candidates(&context, prefix, positional, options)
}

fn completion_candidates(
    context: &Context<'_>,
    prefix: &str,
    positional: usize,
    options: bool,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if options {
        for arg in context
            .arguments()
            .filter(|arg| !arg.hidden && !arg.is_positional())
        {
            if let Some(short) = arg.short {
                candidates.push(format!("-{short}"));
            }
            candidates.extend(
                arg.long
                    .into_iter()
                    .chain(arg.aliases.iter().copied())
                    .chain(arg.negate)
                    .map(|name| format!("--{name}")),
            );
        }
        candidates.extend(
            context
                .builtins()
                .into_iter()
                .map(|(name, _)| name.to_owned()),
        );
        if command_available(context.spec, positional) {
            for sub in context.spec.subcommands().filter(|sub| !sub.hidden) {
                candidates.push(sub.name.to_owned());
                candidates.extend(sub.aliases.iter().map(|alias| (*alias).to_owned()));
            }
        }
    }
    if let Some(arg) = context
        .spec
        .arguments()
        .filter(|arg| arg.is_positional())
        .nth(positional)
    {
        candidates.extend(values(arg, prefix, ""));
    }
    candidates.retain(|candidate| candidate.starts_with(prefix));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn command_available(spec: &CommandSpec, positional: usize) -> bool {
    let mut remaining = spec
        .arguments()
        .filter(|arg| arg.is_positional())
        .skip(positional)
        .peekable();
    !remaining
        .peek()
        .is_some_and(|arg| arg.multi || arg.kind == Kind::Trailing)
        && remaining.all(|arg| !arg.required)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_script(shell: &Shell, executable: &Path) -> Result<String, String> {
    let executable = executable.to_str().ok_or("Executable path must be UTF-8")?;
    let executable = shell_quote(executable);
    Ok(match shell {
        Shell::Bash => {
            format!(
                r#"_pound_command_docs_complete() {{
    local candidate word index last
    local join_value=false strip_name=false
    local -a words=()
    for ((index = 1; index <= COMP_CWORD; index++))
    do
        word=${{COMP_WORDS[index]}}
        last=$((${{#words[@]}} - 1))
        if [[ $word == = && $last -ge 0 && ${{words[last]}} == --* ]]
        then
            words[last]+="="
            join_value=true
            if ((index == COMP_CWORD))
            then
                strip_name=true
            fi
        elif [[ $join_value == true ]]
        then
            words[last]+=$word
            join_value=false
            if ((index == COMP_CWORD))
            then
                strip_name=true
            fi
        else
            words+=("$word")
        fi
    done
    COMPREPLY=()
    while IFS= read -r candidate
    do
        if [[ $strip_name == true ]]
        then
            candidate=${{candidate#*=}}
        fi
        COMPREPLY+=("$candidate")
    done < <({executable} __complete -- "${{words[@]}}")
}}
complete -F _pound_command_docs_complete command_docs {executable}
"#
            )
        },
        Shell::Fish => {
            format!(
                r#"function _pound_command_docs_candidates
    set -l words (commandline -opc)
    {executable} __complete -- $words[2..-1] "$(commandline -ct)"
end
complete -c command_docs -f -a '(_pound_command_docs_candidates)'
"#
            )
        },
    })
}

fn run(cli: &Demo) -> Result<(), String> {
    match &cli.shared.commands.command {
        Action::Help { path } => print!("{}", custom_help(Demo::SPEC, path)?),
        Action::Completions { shell } => {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            print!("{}", shell_script(shell, &executable)?);
        },
        Action::Complete { words } => {
            let words = words.iter().map(String::as_str).collect::<Vec<_>>();
            for candidate in complete(Demo::SPEC, &words) {
                println!("{candidate}");
            }
        },
        Action::Download(_) => println!("{cli:?}"),
    }
    Ok(())
}

fn main() {
    match Demo::try_parse_from(
        std::env::args()
            .skip(1)
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str),
    ) {
        Ok(cli) => {
            if let Err(error) = run(&cli) {
                eprintln!("{error}");
                std::process::exit(2);
            }
        },
        Err(error) => error.exit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_parses_nested_flattening_and_global_aliases() {
        let cli = Demo::try_parse_from([
            "get",
            "https://example.invalid",
            "--output-format",
            "json",
            "-vv",
            "--retries",
            "5",
        ])
        .unwrap();
        assert!(matches!(cli.shared.output.format, Format::Json));
        assert_eq!(cli.shared.verbose, 2);
        assert!(matches!(
            cli.shared.commands.command,
            Action::Download(DownloadAction::Fetch {
                network: Network { retries: 5 },
                ..
            })
        ));
        assert_eq!(
            Demo::SPEC.find_short('f').unwrap().possible,
            Some(["text", "json", "toml"].as_slice())
        );
        assert!(Demo::try_parse_from(["cache", "ls"]).is_ok());
    }

    #[test]
    fn candidates_follow_context_and_value_consumption() {
        for words in [vec!["--format", "j"], vec![
            "cache",
            "ls",
            "--output-format",
            "j",
        ]] {
            assert_eq!(complete(Demo::SPEC, &words), ["json"]);
        }
        assert_eq!(complete(Demo::SPEC, &["cache", "c"]), ["clean"]);
        assert_eq!(complete(Demo::SPEC, &["--format=j"]), ["--format=json"]);
        assert_eq!(complete(Demo::SPEC, &["-vfj"]), ["-vfjson"]);
        assert_eq!(complete(Demo::SPEC, &["completions", "f"]), ["fish"]);
        assert_eq!(
            complete(Demo::SPEC, &["fetch", "--retries", "cache", "--f"]),
            ["--format"]
        );
        assert!(complete(Demo::SPEC, &["fetch", "--", "--f"]).is_empty());
        assert!(complete(Demo::SPEC, &["--format", "invalid", "cache", "--trace"]).is_empty());
        let root = complete(Demo::SPEC, &[""]);
        assert!(root.contains(&"get".to_owned()));
        assert!(!root.contains(&"__complete".to_owned()));
        assert!(!root.contains(&"--trace".to_owned()));
        assert!(!complete(Demo::SPEC, &["cache", ""]).contains(&"doctor".to_owned()));
    }

    #[test]
    fn custom_help_exposes_metadata_and_filters_hidden_entries() {
        let help = custom_help(Demo::SPEC, &["get".to_owned()]).unwrap();
        for text in [
            "command_docs fetch",
            "<URL>",
            "--retries",
            "[default 3]",
            "[env POUND_DEMO_RETRIES]",
            "--output-format",
            "[choices text, json, toml]",
            "[global]",
        ] {
            assert!(help.contains(text), "missing {text}");
        }
        assert!(!help.contains("--trace"));
        assert!(!custom_help(Demo::SPEC, &[]).unwrap().contains("__complete"));
        assert!(custom_help(Demo::SPEC, &["unknown".to_owned()]).is_err());
    }

    #[test]
    fn builtin_spellings_are_independent_and_hash_counts_as_version() {
        const SPEC: CommandSpec = CommandSpec::new("demo")
            .hash("abc123")
            .args(&[ArgSpec::new(Kind::Flag).short('h')]);
        let help = custom_help(&SPEC, &[]).unwrap();
        assert!(help.contains("--help"));
        assert!(help.contains("--version"));
        assert_eq!(complete(&SPEC, &["--v"]), ["--version"]);
    }

    #[test]
    fn subcommands_follow_finite_parent_positionals() {
        const CHILD: CommandSpec =
            CommandSpec::new("run").args(&[ArgSpec::new(Kind::Flag).long("force")]);
        const REQUIRED: CommandSpec = CommandSpec::new("demo")
            .args(&[ArgSpec::new(Kind::Positional)
                .value_name("PROJECT")
                .required()])
            .subs(&[pound::SubSpec::new("run", &CHILD)]);
        const OPTIONAL: CommandSpec = CommandSpec::new("demo")
            .args(&[ArgSpec::new(Kind::Positional).value_name("PROJECT")])
            .subs(&[pound::SubSpec::new("run", &CHILD)]);
        const VARIADIC: CommandSpec = CommandSpec::new("demo")
            .args(&[ArgSpec::new(Kind::Positional).value_name("PROJECT").multi()])
            .subs(&[pound::SubSpec::new("run", &CHILD)]);

        assert!(complete(&REQUIRED, &["r"]).is_empty());
        assert_eq!(complete(&REQUIRED, &["project", "r"]), ["run"]);
        assert_eq!(complete(&REQUIRED, &["project", "run", "--f"]), ["--force"]);
        assert_eq!(complete(&OPTIONAL, &["r"]), ["run"]);
        assert_eq!(complete(&OPTIONAL, &["project", "r"]), ["run"]);
        assert_eq!(complete(&OPTIONAL, &["run", "--f"]), ["--force"]);
        assert!(complete(&VARIADIC, &["project", "r"]).is_empty());
        assert!(complete(&REQUIRED, &["project", "--", "r"]).is_empty());
    }

    #[test]
    fn nearest_global_supplies_completion_values() {
        const LEAF: CommandSpec = CommandSpec::new("leaf");
        const CHILD: CommandSpec = CommandSpec::new("child")
            .args(&[ArgSpec::new(Kind::Opt)
                .long("mode")
                .short('m')
                .aliases(&["style"])
                .possible(&["child"])
                .global()])
            .subs(&[pound::SubSpec::new("leaf", &LEAF)]);
        const ROOT: CommandSpec = CommandSpec::new("root")
            .args(&[ArgSpec::new(Kind::Opt)
                .long("mode")
                .short('m')
                .aliases(&["style"])
                .possible(&["root"])
                .global()])
            .subs(&[pound::SubSpec::new("child", &CHILD)]);

        assert_eq!(complete(&ROOT, &["--mode", ""]), ["root"]);
        for spelling in ["--mode", "--style", "-m"] {
            assert_eq!(complete(&ROOT, &["child", spelling, ""]), ["child"]);
            assert_eq!(complete(&ROOT, &["child", "leaf", spelling, ""]), ["child"]);
        }
        assert_eq!(complete(&ROOT, &["child", "leaf", "--mode=c"]), [
            "--mode=child"
        ]);
        assert_eq!(complete(&ROOT, &["child", "leaf", "-mc"]), ["-mchild"]);
    }
}
