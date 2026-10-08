# screw

differential terminal renderer, a tree of widgets drawn by writing only what changed since the
previous frame

```rust,no_run
use screw::{Runtime, Text, widget};

fn main() -> std::io::Result<()> {
    let runtime = Runtime::stderr(widget(Text::new("working"))).start();
    runtime.mark_dirty()?;
    runtime.finish()?;
    Ok(())
}
```

EUPL-1.2
