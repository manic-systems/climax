# bang-terminal

terminal session toolkit behind `bang`, with raw mode, screen and signal guards and an input
decoder, an implementation contract versioned with `bang`, reached as `bang::terminal`

```rust
use bang_terminal::Decoder;

fn main() {
    let decoder = Decoder::new();
    drop(decoder);
}
```

EUPL-1.2
