use std::{cell::RefCell, rc::Rc, time::Instant};

use screw::{RenderCtx, Runtime, Stack, Style, Surface, Widget, local_widget, widget};

struct LocalView(Rc<RefCell<String>>);

impl Widget for LocalView {
    fn render(&self, _context: &RenderCtx, output: &mut Surface) {
        output.write(&*self.0.borrow(), Style::PLAIN);
    }
}

fn main() {
    let rendered = screw::render_plain(&"standalone screw");
    assert_eq!(rendered, "standalone screw");

    let value = Rc::new(RefCell::new("local state".to_owned()));
    let root = local_widget(Stack::new(vec![local_widget(LocalView(value))]));
    Runtime::new(Vec::new(), root)
        .draw_now(Instant::now())
        .unwrap();

    Runtime::new(Vec::new(), widget("threaded state"))
        .start()
        .finish()
        .unwrap();
}
