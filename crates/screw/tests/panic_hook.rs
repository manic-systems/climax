// SPDX-License-Identifier: EUPL-1.2

use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use std::panic;

use screw::{
    RenderCtx,
    Runtime,
    Surface,
    Widget,
};

static HOOK_CALLS: AtomicUsize = AtomicUsize::new(0);

struct Exploding;

impl Widget for Exploding {
    #[expect(clippy::panic, reason = "the widget exists to panic")]
    fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {
        panic!("widget exploded");
    }
}

#[test]
fn a_widget_panic_reaches_the_panic_hook_once_and_finish_does_not_repeat_it() {
    panic::set_hook(Box::new(|_| {
        HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    }));

    let live = Runtime::new(Vec::new(), Exploding).start();
    let error = live.finish().unwrap_err();
    drop(panic::take_hook());

    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(error.to_string(), "a widget panicked on the render thread");
    assert!(!error.to_string().contains("widget exploded"), "{error}");
}
