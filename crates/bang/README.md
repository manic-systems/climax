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

## prompts

| prompt         | answer                | initial value     |
|----------------|-----------------------|-------------------|
| `select`       | one choice            | `selected(index)` |
| `multi_select` | any number of choices | `checked(index)`  |
| `search`       | one choice, filtered  | `selected(index)` |
| `review`       | items with a state    | `selected(index)` |
| `text`         | `String`              | `default(text)`   |
| `password`     | `String`, masked      | `default(text)`   |
| `number::<T>`  | any `T: FromStr`      | `default(value)`  |
| `confirm`      | `bool`                | `default(bool)`   |
| `date`         | `Date`                | `default(date)`   |

every prompt takes `id`, `summary` and `interaction`. `text`, `password` and `number` take a
`validator` and a `placeholder`. `interact` returns `PromptOutcome`, and `or_cancel` turns a left
prompt into an `ErrorKind::Cancelled` error so `?` ends the program

prompts draw on stderr and read stdin, so stdout stays free for the answer. a submitted prompt leaves
a dim summary line such as `Shell › bash` in the scrollback, off with `summary(false)`

## testing

`bang::advanced::scripted_interaction` takes one list of key events per prompt and drives the
prompts without a terminal. dropping it with events left unread panics, and
`checked_scripted_interaction` asks instead

EUPL-1.2
