use std::{
    io,
    sync::Arc,
    thread,
    time::Duration,
};

use screw::{
    Color,
    InputAnchor,
    Looping,
    ProgressBar,
    Runtime,
    Stateful,
    Style,
    Text,
    WidgetRef,
    WindowedLines,
    layout,
    widget,
};

const SCENE_FRAMES: usize = 4;
const SCENE_FRAME: Duration = Duration::from_millis(180);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum LoadState {
    Early,
    Slow,
    Done,
}

struct Demo {
    root:     WidgetRef,
    state:    Arc<Stateful<LoadState>>,
    logs:     WindowedLines,
    progress: ProgressBar,
}

impl Demo {
    fn new() -> Self {
        let state = Arc::new(
            Stateful::new(LoadState::Early)
                .case(
                    LoadState::Early,
                    widget(
                        Looping::new(["/", "-", "\\", "|"]).style(Style::default().fg(Color::Blue)),
                    ),
                )
                .case(
                    LoadState::Slow,
                    widget(
                        Looping::new(["/", "-", "\\", "|"])
                            .style(Style::default().fg(Color::Yellow)),
                    ),
                )
                .case(
                    LoadState::Done,
                    widget(Text::new("v").style(Style::default().fg(Color::Green))),
                ),
        );
        let logs = WindowedLines::new(5).style(Style::default().dim());
        let progress = ProgressBar::new(24).styles(
            Style::default().fg(Color::Blue),
            Style::default().fg(Color::White).dim(),
        );
        let root = layout()
            .line(vec![
                widget(Arc::clone(&state)),
                widget(Text::new(" building environment")),
            ])
            .widget(widget(logs.clone()))
            .widget(widget(progress.clone()))
            .input(InputAnchor::prompt("> "))
            .into_widget();

        Self {
            root,
            state,
            logs,
            progress,
        }
    }
}

fn main() -> io::Result<()> {
    let demo = Demo::new();
    let runtime = Runtime::stderr_auto(demo.root).fps(12).start();

    linger(&runtime)?;

    demo.logs.push("copying path /nix/store/example-a");
    demo.progress.set_fraction(0.25);
    runtime.mark_dirty()?;
    linger(&runtime)?;

    demo.logs.push("building crate screw");
    demo.progress.set_fraction(0.50);
    demo.state.set_state(LoadState::Slow);
    runtime.mark_dirty()?;
    linger(&runtime)?;

    demo.logs.push("finished build");
    demo.progress.set_fraction(1.0);
    demo.state.set_state(LoadState::Done);
    runtime.mark_dirty()?;
    let _stderr = runtime.finish()?;

    Ok(())
}

fn linger<W>(runtime: &screw::AutoRuntime<W>) -> io::Result<()>
where
    W: io::Write + Send + 'static,
{
    if !screw::stderr_is_terminal() {
        return Ok(());
    }

    for _ in 0..SCENE_FRAMES {
        thread::sleep(SCENE_FRAME);
        runtime.mark_dirty()?;
    }
    Ok(())
}
