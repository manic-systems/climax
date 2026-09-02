use std::io;

use screw::{CursorMerge, Edge, Fill, Floating, Insets, Layers, Renderer, Size, Style};

fn main() -> io::Result<()> {
    let document = "Document content keeps its full layout width.\n\nThe pane covers these cells.";
    let actions = "┌─ Actions ──────────┐\n│ q  quit           │\n│ ?  help           │\n└───────────────────┘";
    let frame = Layers::new(document).float(
        actions,
        Floating::new(Edge::BOTTOM | Edge::RIGHT)
            .margin(Insets::bottom(1))
            .max_size(Size::new(32, 8))
            .fill(Fill::Opaque(Style::PLAIN))
            .cursor(CursorMerge::PreserveBase),
    );

    Renderer::stderr().height(10).draw(&frame)?;
    Ok(())
}
