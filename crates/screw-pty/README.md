# screw-pty

`screw-pty` is the PTY-side adapter and emitted-output test harness for `screw`.

it owns the future path from child process terminal output into `screw::Surface`
and `screw::Widget` values. it must stay independent of `bang` and `climax`.

current scope:

- provide a deliberately plain line-frame `screw::Widget` bridge for callers
  which already own decoded text
- interpret the focused ANSI alphabet emitted by Screw and Climax for
  headless acceptance tests
- reject unsupported escape sequences instead of silently approximating them
- leave process spawning and general terminal emulation to later work

`EmittedScreen` intentionally covers printable UTF-8, CR/LF, Screw's relative
cursor movement and line erasure, its SGR subset, cursor visibility, alternate
screen, and bracketed-paste modes. It is not an emulator for arbitrary child
applications.

There is no lossy `PtyScreen` byte collector: raw terminal output must either
pass through `EmittedScreen` for the supported acceptance-test alphabet or a
future real terminal emulator before it is presented as cells.
