# screw-pty

`screw-pty` is a strict model of the terminal output that `screw` emits, used to
test `screw` and the crates built on it. It is not published.

`EmittedScreen` consumes the bytes written by a `screw::Renderer` and rebuilds
the screen they produce, so a test can assert on cells, styles and cursor
state. It covers printable UTF-8, CR/LF, relative cursor movement, line erasure
and erase-below, the SGR attributes and colours screw emits, cursor visibility,
the alternate screen and bracketed paste. Any other sequence is rejected with a
`ScreenError` instead of being approximated.

It is not an emulator for arbitrary child applications.

`EmittedScreen` joins characters into cells with the same `unicode-segmentation`
and `unicode-width` crates that `screw` measures with. A test through it
confirms the renderer agrees with its own width policy and cannot detect a
policy error, so check widths against an independent source as well.
