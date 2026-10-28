# bang-run

command-line tool that runs one `bang` prompt and prints the answer, installed as `bang`

```text
bang select --option bash --option zsh
bang --output json text --value ada
bang --config prompt.toml
```

`-o` is `--output`, `text` or `json`, and `--option` has no short flag. exit codes are 0 on success,
2 for usage or config errors, 130 on cancellation, 128 plus the signal number on a signal, 141 when
stdout is a closed pipe and 1 otherwise, an unreadable config file included

the prompt uses stdin and stderr, and opens `/dev/tty` instead when either is redirected and the
process has a controlling terminal

## Input and actions

`--input-bytes` replays bytes instead of reading input, with the escapes `\n`, `\r`, `\t`, `\e`, `\\`
and `\xHH`

`--action key:name` binds a key. the key is one character, `ctrl-` and a letter, or `enter`, `esc`,
`tab`, `backtab`, `shift-tab`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`,
`page-up` or `page-down`. `ctrl-m`, `ctrl-i`, `ctrl-j` and `ctrl-h` are refused, since terminals
send them as enter, tab and backspace. pressing the key submits `{"action":"name","value":...}`
with the value so far, and a bound character can no longer be typed. on `review-list` the key is one
character and the result is the review result with `"exit":"action"` and `"action":"name"`

## Config

`--config` reads a toml file instead of a subcommand. `type` picks the widget, and unknown keys and
keys that do not apply to the type are rejected

| key             | applies to         | meaning                                                                     |
|-----------------|--------------------|-----------------------------------------------------------------------------|
| `type`          | all                | `select`, `multi-select`, `text`, `search`, `form`, `date` or `review-list` |
| `prompt`        | all                | prompt text shown above the widget                                          |
| `placeholder`   | `text`, `search`   | text shown while empty                                                      |
| `value`         | `text`             | initial value                                                               |
| `options`       | list types         | labels, or tables with `label`, `value`, `selected` and `state`             |
| `page_size`     | list types         | visible rows, at least 1                                                    |
| `wrap`          | list types         | wrap the cursor at either end                                               |
| `show_removed`  | `review-list`      | list rows that start denied                                                 |
| `action_output` | `review-list`      | return a structured exit and rows                                           |
| `actions`       | all                | tables with `key`, `name` and optional `help`, keys as for `--action`       |
| `selected_date` | `date`             | initial date, `YYYY-MM-DD`                                                  |
| `today`         | `date`             | date treated as today, `YYYY-MM-DD`                                         |
| `fields`        | `form`             | tables with a unique `name`, a `type` and that type's keys                  |
| `input_bytes`   | all                | escaped terminal bytes for a run without a terminal                         |

list types are `select`, `multi-select`, `search` and `review-list`, and need at least one option.
`date` needs `selected_date` and `form` needs at least one field. a field is any type but `form`,
names and option labels cannot be empty, and an option `state` is `unconfirmed`, `confirmed` or
`denied` and only applies to `review-list`

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
