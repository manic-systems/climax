// SPDX-License-Identifier: EUPL-1.2

use std::sync::{
    Arc,
    atomic::{
        AtomicUsize,
        Ordering,
    },
};

use climax::{
    prelude::*,
    screw::{
        RenderCtx,
        Style,
        Surface,
        Widget,
        widget,
    },
};

#[climax::serde(Serialize)]
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Shell {
    Bash,
    Zsh,
}

/// configure a shell
#[derive(Parse)]
struct Args {
    /// shell to configure, asked for when absent
    #[pound(long)]
    shell: Option<Shell>,
}

struct Progress(Arc<AtomicUsize>);

impl Widget for Progress {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(
            format!("{} of 3", self.0.load(Ordering::Relaxed)),
            Style::PLAIN,
        );
    }
}

fn run(context: &Context, args: &Args) -> climax::Result<()> {
    let shell = match args.shell {
        Some(shell) => shell,
        None => {
            match context
                .select("shell")
                .choice("bash", Shell::Bash)
                .choice("zsh", Shell::Zsh)
                .interact()?
            {
                PromptOutcome::Submit(shell) => shell,
                PromptOutcome::Leave => return Ok(()),
            }
        },
    };

    let done = Arc::new(AtomicUsize::new(0));
    let status = context
        .status("writing configuration")
        .spinner()
        .widget(widget(Progress(Arc::clone(&done))))
        .start();
    for _ in 0..3 {
        done.fetch_add(1, Ordering::Relaxed);
        status.mark_dirty()?;
    }
    status.finish()?;

    context
        .output()
        .result(&shell)
        .text(|shell| format!("{shell:?}"))
        .emit()
}

fn main() -> std::process::ExitCode {
    climax::main(|context, args: Args| run(&context, &args))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        Mutex,
    };

    use climax::bang::advanced::{
        Event,
        Key,
        scripted_interaction,
    };

    use super::*;

    #[climax::serde(Serialize, Deserialize)]
    #[derive(Debug, PartialEq)]
    #[serde(rename_all = "kebab-case")]
    struct Report {
        shell_name: String,
        #[serde(rename = "lines")]
        line_count: u32,
    }

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_serde_attribute_roots_derives_and_keeps_helpers() {
        let report = Report {
            shell_name: "zsh".to_owned(),
            line_count: 3,
        };
        let buffer = Buffer::default();
        let context = Context::new()
            .with_output_format(Format::Json)
            .with_output_writer(buffer.clone());
        context
            .output()
            .stream(&report)
            .text(|_| String::new())
            .emit()
            .unwrap();
        drop(context);
        assert_eq!(
            String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap(),
            "{\"shell-name\":\"zsh\",\"lines\":3}\n",
        );
        fn deserializes<T: for<'de> climax::serde::Deserialize<'de>>() {}
        deserializes::<Report>();
    }

    #[test]
    fn a_scripted_prompt_drives_the_application() {
        let interaction = scripted_interaction([[Event::key(Key::Enter)]]);
        let context = Context::new().with_interaction(interaction);
        run(&context, &Args { shell: None }).unwrap();
    }
}
