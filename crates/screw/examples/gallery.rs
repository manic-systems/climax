use std::{
    io,
    sync::{
        Arc,
        Mutex,
    },
    thread,
    time::Duration,
};

use screw::{
    Color,
    Grid,
    GridCell,
    Line,
    List,
    Looping,
    ProgressBar,
    RenderCtx,
    Role,
    Runtime,
    Stack,
    Stateful,
    Style,
    Surface,
    Text,
    TextInput,
    Widget,
    WidgetRef,
    WindowedLines,
    screw,
    widget,
};

const SCENE_FRAMES: usize = 6;
const SCENE_FRAME: Duration = Duration::from_millis(180);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Phase {
    Starting,
    Busy,
    Done,
}

#[derive(Clone)]
struct SelectedList {
    rows:     Vec<String>,
    selected: Arc<Mutex<usize>>,
}

impl SelectedList {
    fn new(rows: &[&str]) -> Self {
        Self {
            rows:     rows.iter().map(ToString::to_string).collect(),
            selected: Arc::new(Mutex::new(0)),
        }
    }

    fn select(&self, selected: usize) {
        *self.selected.lock().expect("selected-list mutex poisoned") = selected;
    }
}

impl Widget for SelectedList {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let selected = *self.selected.lock().expect("selected-list mutex poisoned");
        List::new(self.rows.clone())
            .height(4)
            .selected(selected)
            .render(ctx, out);
    }
}

#[derive(Clone)]
struct Calendar {
    selected: Arc<Mutex<usize>>,
}

impl Calendar {
    fn new() -> Self {
        Self {
            selected: Arc::new(Mutex::new(24)),
        }
    }

    fn select(&self, day: usize) {
        *self.selected.lock().expect("calendar mutex poisoned") = day;
    }
}

impl Widget for Calendar {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let selected = *self.selected.lock().expect("calendar mutex poisoned");
        let rows = [["20", "21", "22", "23", "24", "25", "26"], [
            "27", "28", "29", "30", "31", "  ", "  ",
        ]]
        .into_iter()
        .map(|week| {
            week.into_iter()
                .map(|day| {
                    let role = day.trim().parse::<usize>().ok().map_or(Role::Dim, |value| {
                        if value == selected {
                            Role::Selected
                        } else {
                            Role::Normal
                        }
                    });
                    GridCell::new(day).role(role)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

        Grid::new(rows).gap(2).render(ctx, out);
    }
}

#[derive(Clone)]
struct DraftInput {
    value: Arc<Mutex<String>>,
}

impl DraftInput {
    fn new() -> Self {
        Self {
            value: Arc::new(Mutex::new(String::new())),
        }
    }

    fn set(&self, value: &str) {
        *self.value.lock().expect("draft-input mutex poisoned") = value.to_string();
    }
}

impl Widget for DraftInput {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let value = self.value.lock().expect("draft-input mutex poisoned");
        TextInput::new("filter: ", value.as_str())
            .cursor(value.chars().count())
            .roles(Role::Prompt, Role::Normal)
            .render(ctx, out);
    }
}

struct Gallery {
    root:     WidgetRef,
    phase:    Arc<Stateful<Phase>>,
    logs:     WindowedLines,
    progress: ProgressBar,
    list:     SelectedList,
    calendar: Calendar,
    input:    DraftInput,
}

impl Gallery {
    fn new() -> Self {
        let phase = Arc::new(
            Stateful::new(Phase::Starting)
                .case(
                    Phase::Starting,
                    widget(
                        Looping::new(["/", "-", "\\", "|"]).style(Style::default().fg(Color::Blue)),
                    ),
                )
                .case(
                    Phase::Busy,
                    widget(
                        Looping::new(["⠁", "⠂", "⠄", "⠂"])
                            .style(Style::default().fg(Color::Yellow).bold()),
                    ),
                )
                .case(
                    Phase::Done,
                    widget(Text::new("v").style(Style::default().fg(Color::Green).bold())),
                ),
        );
        let logs = WindowedLines::new(5).style(Style::default().dim());
        let progress = ProgressBar::new(24).styles(
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::White).dim(),
        );
        let list = SelectedList::new(&[
            "download metadata",
            "resolve graph",
            "build package",
            "write profile",
            "activate shell",
            "summarize",
        ]);
        let calendar = Calendar::new();
        let input = DraftInput::new();

        let status = widget(Line::new(vec![
            widget(Arc::clone(&phase)),
            widget(Text::new(" screw differential gallery")),
        ]));
        #[allow(clippy::literal_string_with_formatting_args)]
        let root = screw!(
            "{status}\n{logs}\n{progress}\n{list}\n{calendar}\n{input}",
            status = status,
            logs = logs.clone(),
            progress = progress.clone(),
            list = list.clone(),
            calendar = calendar.clone(),
            input = input.clone(),
        );

        Self {
            root: widget(root),
            phase,
            logs,
            progress,
            list,
            calendar,
            input,
        }
    }

    fn final_summary() -> WidgetRef {
        widget(Stack::new(vec![
            widget(Text::new("screw gallery complete").role(Role::Success)),
            widget(Text::new(
                "covered: spinner, logs, progress, list, grid, input, clipping",
            )),
        ]))
    }

    fn step<W>(&self, runtime: &screw::AutoRuntime<W>, step: usize) -> io::Result<()>
    where
        W: io::Write + Send + 'static,
    {
        match step {
            0 => {
                self.logs.push("start: initial render with empty widgets");
                self.progress.set_fraction(0.10);
                self.input.set("d");
            },
            1 => {
                self.logs.push("log: only the windowed tail should grow");
                self.progress.set_fraction(0.25);
                self.list.select(1);
                self.input.set("de");
            },
            2 => {
                self.logs
                    .push("status: spinner changes without repainting logs");
                self.phase.set_state(Phase::Busy);
                self.progress.set_fraction(0.50);
                self.list.select(2);
                self.calendar.select(30);
                self.input.set("dep");
            },
            3 => {
                self.logs.push(
                    "wide: this deliberately long line is clipped instead of soft-wrapped by the \
                     renderer",
                );
                self.progress.set_fraction(0.75);
                self.list.select(4);
                self.input.set("depe");
            },
            4 => {
                self.logs
                    .push("done: final summary will replace the transient frame");
                self.progress.set_fraction(1.0);
                self.phase.set_state(Phase::Done);
                self.list.select(5);
                self.calendar.select(31);
                self.input.set("depend");
            },
            _ => {},
        }
        runtime.mark_dirty()
    }
}

fn main() -> io::Result<()> {
    let gallery = Gallery::new();
    let runtime = Runtime::stderr_auto(gallery.root.clone())
        .fps(12)
        .final_widget(Gallery::final_summary())
        .start();

    for step in 0..5 {
        gallery.step(&runtime, step)?;
        linger(&runtime)?;
    }

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
