# bang-core

widget, event, value and session model behind `bang`, an implementation contract versioned with
`bang`, applications should depend on `bang` instead

```rust
use bang_core::widgets::TextInput;

fn main() {
    let input = TextInput::new("name").with_value("ada");
    drop(input);
}
```

EUPL-1.2
