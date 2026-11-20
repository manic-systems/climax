# screw

differential terminal renderer, a tree of widgets drawn by writing only what changed since the
previous frame

```rust,no_run
use screw::{Runtime, Text, widget};

fn main() -> std::io::Result<()> {
    let mut runtime = Runtime::stderr(widget(Text::new("working"))).start();
    runtime.println("started")?;
    runtime.finish()?;
    Ok(())
}
```

`Runtime::stderr` draws live on a terminal and plain text anywhere else, `Runtime::stderr_live` always
draws live. `finish` ends the frame with a newline on a terminal, and `println` prints a line above
the frame, since writing to stderr another way garbles it. a live runtime follows terminal resizes.
the `examples` directory has runnable programs, screw is unix only

EUPL-1.2
