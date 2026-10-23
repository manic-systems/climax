// SPDX-License-Identifier: EUPL-1.2

use bang::advanced::widgets::TextInput;
use screw::{
    Position,
    RenderCtx,
    Role,
    Style,
    Surface,
    Theme,
    Widget as _,
};

#[test]
fn a_widget_renders_into_a_screw_surface() {
    let widget = TextInput::new("name").with_prompt("name: ");
    let mut surface = Surface::new();

    widget.render(&RenderCtx::new(), &mut surface);

    assert_eq!(surface.plain_text(), "name: ");
    assert_eq!(surface.cursor(), Some(Position { row: 0, col: 6 }));
}

#[test]
fn a_widget_continues_the_row_it_is_rendered_onto() {
    let widget = TextInput::new("name").with_prompt("name: ");
    let mut surface = Surface::new();
    surface.write("[", Style::default());

    widget.render(&RenderCtx::new(), &mut surface);

    assert_eq!(surface.plain_text(), "[name: ");
}

#[test]
fn roles_resolve_through_the_theme_of_the_render_context() {
    let widget = TextInput::new("name").with_prompt("name: ");
    let theme = Theme::default();
    let mut surface = Surface::new();

    widget.render(&RenderCtx::new().with_theme(theme), &mut surface);

    assert_eq!(
        surface.rows()[0].cells()[0].style(),
        theme.style(Role::Prompt)
    );
}
