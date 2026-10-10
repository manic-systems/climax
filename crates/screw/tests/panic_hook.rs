// SPDX-License-Identifier: EUPL-1.2

use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use std::{
    panic,
    thread,
};

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
fn a_widget_panic_returned_by_finish_is_not_also_reported_by_the_panic_hook() {
    panic::set_hook(Box::new(|_| {
        HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    }));

    let live = Runtime::new(Vec::new(), Exploding).start();
    let error = live.finish().unwrap_err();
    assert!(error.to_string().contains("widget exploded"), "{error}");
    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 0);

    drop(thread::spawn(|| panic!("unrelated")).join());
    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 1);
}
