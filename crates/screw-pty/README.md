# screw-pty

strict model of the terminal output `screw` emits, for testing. not published

`EmittedScreen` rebuilds the screen from the bytes a `screw::Renderer` writes, and any other sequence
besides bracketed paste and the alternate screen is a `ScreenError`
