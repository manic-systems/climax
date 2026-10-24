# bang-run

command-line tool that runs one `bang` prompt and prints the answer, installed as `bang`

```text
bang select --option bash --option zsh
bang --output json text --value ada
bang --config prompt.toml
```

`-o` is `--output`, `text` or `json`, and `--option` has no short flag. exit codes are 0 on success,
2 for usage or config errors, 130 on cancellation, 128 plus the signal number on a signal, 141 when
stdout is a closed pipe and 1 otherwise

## Config

`--config` reads a toml file instead of a subcommand. unknown keys are rejected, `type` picks the
widget and the other keys apply per type

| key             | applies to    | meaning                                                                     |
|-----------------|---------------|-----------------------------------------------------------------------------|
| `type`          | all           | `select`, `multi-select`, `text`, `search`, `form`, `date` or `review-list` |
| `prompt`        | all           | prompt text                                                                 |
| `placeholder`   | `text`        | text shown while empty                                                      |
| `value`         | `text`        | initial value                                                               |
| `options`       | list types    | labels, or tables with `label`, `value`, `selected` and `state`             |
| `page_size`     | list types    | visible rows, at least 1                                                    |
| `wrap`          | list types    | wrap the cursor at either end                                               |
| `show_removed`  | `review-list` | show denied rows                                                            |
| `action_output` | `review-list` | return a structured exit and rows                                           |
| `actions`       | all           | tables with `key`, `name` and optional `help`                               |
| `selected_date` | `date`        | initial date, `YYYY-MM-DD`                                                  |
| `today`         | `date`        | date treated as today, `YYYY-MM-DD`                                         |
| `fields`        | `form`        | tables with a unique `name`, a `type` and that type's keys                  |
| `input_bytes`   | all           | escaped terminal bytes for a run without a terminal                         |

list types need at least one option, `date` needs `selected_date` and `form` needs at least one
field. a field is any type but `form`, names and option labels cannot be empty, and `state` is
`unconfirmed`, `confirmed` or `denied`

```toml
type = "form"
prompt = "new user"

[[fields]]
name = "who"
type = "text"
placeholder = "name"

[[fields]]
name = "shell"
type = "select"
options = ["bash", { label = "zsh", selected = true }]

[[fields]]
name = "since"
type = "date"
selected_date = "2024-02-29"
```

EUPL-1.2
