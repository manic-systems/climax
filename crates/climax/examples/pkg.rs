// SPDX-License-Identifier: EUPL-1.2

use core::time::Duration;
use std::{
    process::ExitCode,
    thread,
};

use climax::prelude::*;

#[climax::serde(Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Lib,
    Bin,
}

#[climax::serde(Serialize)]
struct Package {
    name: String,
    kind: Kind,
}

/// a tiny package manager
#[derive(Parse)]
struct Cli {
    /// print results as json
    #[pound(long, global)]
    json:    bool,
    /// when to colour output
    #[pound(long, global, default = auto)]
    color:   ColorChoice,
    #[pound(subcommand)]
    command: Command,
}

#[derive(Parse)]
enum Command {
    /// install a package
    Add {
        /// package name
        name: String,
        /// what the package builds
        #[pound(long, default = lib)]
        kind: Kind,
    },
    /// remove a package after asking
    Remove {
        /// package name
        name: String,
        /// skip the question
        #[pound(short, long)]
        yes:  bool,
    },
    /// list the installed packages
    List,
}

fn main() -> ExitCode {
    climax::main(run)
}

fn run(mut cx: Context, cli: Cli) -> climax::Result<()> {
    if cli.json {
        cx.set_output_format(Format::Json);
    }
    cx.set_color(cli.color);
    match cli.command {
        Command::Add { name, kind } => add(&cx, name, kind),
        Command::Remove { name, yes } => remove(&cx, &name, yes),
        Command::List => list(&cx),
    }
}

fn add(cx: &Context, name: String, kind: Kind) -> climax::Result<()> {
    let progress = cx
        .status(format!("installing {name}"))
        .final_message(format!("installed {name}"))
        .progress(5);
    for _ in 0..5 {
        thread::sleep(Duration::from_millis(120));
        progress.inc(1)?;
    }
    progress.finish()?;
    let package = Package { name, kind };
    cx.output()
        .result(&package)
        .text(|pkg| format!("{} ({})", pkg.name, pkg.kind.as_str()))
        .emit()
}

fn remove(cx: &Context, name: &str, yes: bool) -> climax::Result<()> {
    if !yes
        && !cx
            .confirm(format!("remove {name}?"))
            .interact()?
            .or_cancel()?
    {
        return Err(Error::cancelled());
    }
    cx.output().notice(format!("removed {name}"))
}

fn list(cx: &Context) -> climax::Result<()> {
    for (name, kind) in [("pound", Kind::Lib), ("bang-run", Kind::Bin)] {
        let package = Package {
            name: name.to_owned(),
            kind,
        };
        cx.output()
            .stream(&package)
            .text(|pkg| format!("{} ({})", pkg.name, pkg.kind.as_str()))
            .emit()?;
    }
    Ok(())
}
