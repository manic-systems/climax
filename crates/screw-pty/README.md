# screw-pty

`screw-pty` is the PTY-side adapter for `screw`.

it owns the future path from child process terminal output into `screw::Surface`
and `screw::Widget` values. it must stay independent of `bang` and `climax`.

current scope:

- represent a PTY screen as renderable lines
- provide the smallest `screw::Widget` bridge
- leave process spawning and terminal emulation details for the next pass
