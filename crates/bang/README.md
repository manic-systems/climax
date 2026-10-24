# bang

typed interactive prompts for the terminal, layered over `screw`

```rust,no_run
use bang::{
    PromptOutcome,
    select,
};

fn main() -> bang::Result<()> {
    let outcome = select("Pick a shell")
        .choice("Bash", "bash")
        .choice("Zsh", "zsh")
        .interact()?;
    if let PromptOutcome::Submit(shell) = outcome {
        println!("chose {shell}");
    }
    Ok(())
}
```

EUPL-1.2
