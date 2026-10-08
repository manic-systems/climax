# screw

screw is a differential terminal renderer. You describe a frame as a tree of
widgets, and screw keeps the previous frame and writes only the control
sequences needed to turn it into the next one. That makes animated status
lines, spinners, progress bars and small interactive panes cheap and free of
flicker.

It runs live on a terminal. `Runtime::auto` takes an `interactive` flag from the
caller and falls back to plain text when it is false, and `Runtime::stderr_auto`
sets the flag from whether standard error is a terminal, so the same program
reads well in a CI log.

## Install

```sh
cargo add screw
```

## A widget

A widget writes styled text into a surface. State that changes while it is on
screen lives behind a shared handle, and the runtime is told to redraw when it
changes.

```rust,no_run
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use screw::{Color, RenderCtx, Runtime, Style, Surface, Widget, widget};

#[derive(Clone)]
struct Download {
    received: Arc<AtomicU64>,
    total: u64,
}

impl Widget for Download {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        let received = self.received.load(Ordering::Relaxed);
        let style = if received >= self.total {
            Style::new().fg(Color::Green)
        } else {
            Style::new()
        };
        out.write(format!("{received}/{} bytes", self.total), style);
    }
}

fn main() -> std::io::Result<()> {
    let download = Download {
        received: Arc::new(AtomicU64::new(0)),
        total: 300,
    };
    let runtime = Runtime::stderr(widget(download.clone())).start();

    for step in 1..=3 {
        download.received.store(step * 100, Ordering::Relaxed);
        runtime.mark_dirty()?;
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    runtime.finish()?;
    Ok(())
}
```

Built-in widgets cover text with mixed styles (`Spans`), aligned columns
(`Table`), spinners (`Looping`), progress bars (`ProgressBar`), lists (`List`),
text input (`TextInput`) and floating panes (`Layers`). `screw::width`,
`screw::truncate` and `screw::pad` measure text the same way the renderer lays
it out, for widgets of your own. They count each grapheme cluster once and
expand a tab to the next multiple of eight columns, under the width policy in
the crate docs. `Cell::new` takes a `&str` and returns `None` unless it is
exactly one grapheme cluster. `Looping::new` takes any iterator of string-like
frames, and `LiveRuntime::finish_recovering` hands the writer back when a draw
fails or a widget panics.

## Examples

The [examples](https://github.com/manic-systems/climax/tree/main/crates/screw/examples)
directory has a build-progress display (`cade`), a tour of the built-in widgets
(`gallery`) and floating panes (`layers`). Run one with
`cargo run -p screw --example gallery`.

## License

EUPL-1.2
