// SPDX-License-Identifier: EUPL-1.2

#![expect(
    clippy::non_ascii_literal,
    reason = "terminal glyphs read better as themselves than as escapes"
)]

use bang_core::{
    Context,
    Date,
    Event,
    Key,
    KeyEvent,
    Modifiers,
    Reaction,
    Session,
    Widget as _,
    widgets::{
        DatePicker,
        Form,
        MultiSelect,
        ReviewList,
        SearchSelect,
        Select,
        TextInput,
    },
};
use screw::{
    Position,
    RenderCtx,
    Role,
    Surface,
    Theme,
    Widget,
};

fn draw(widget: &impl Widget, rows: Option<usize>) -> Surface {
    let mut surface = Surface::new();
    let ctx = RenderCtx::new().with_constraints(Some(40), rows);
    widget.render(&ctx, &mut surface);
    surface
}

fn lines(widget: &impl Widget, rows: usize) -> Vec<String> {
    draw(widget, Some(rows))
        .plain_text()
        .lines()
        .map(str::to_owned)
        .collect()
}

const fn page_key(key: Key) -> Event {
    Event::Key(KeyEvent {
        key,
        modifiers: Modifiers::empty(),
    })
}

fn items(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("item-{index:02}")).collect()
}

#[expect(clippy::expect_used, reason = "the test dates are valid")]
const fn date(day: u8) -> Date {
    Date::new(2026, 7, day).expect("test date is valid")
}

#[test]
fn a_list_keeps_plain_content_and_semantic_styles() {
    let multi = MultiSelect::new("choices", ["Alpha", "Beta"])
        .with_header("Choose")
        .with_checked_indices([0]);

    let mut surface = Surface::new();
    multi.render(
        &RenderCtx::new().with_constraints(Some(80), None),
        &mut surface,
    );
    assert_eq!(
        surface.plain_text(),
        "Choose\n> [x] Alpha\n  [ ] Beta\nspace toggle | a all | n none | up/down move | enter \
         submit | esc cancel"
    );

    let theme = Theme::default();
    assert_eq!(
        surface.rows()[0].cells()[0].style(),
        theme.style(Role::Prompt)
    );
    assert_eq!(
        surface.rows()[1].cells()[0].style(),
        theme.style(Role::Selected)
    );
    assert_eq!(surface.rows()[2].cells()[0].style(), theme.style(Role::Dim));
    assert_eq!(surface.rows()[3].cells()[0].style(), theme.style(Role::Dim));
}

#[test]
fn a_search_match_is_styled_with_the_match_role() {
    let mut search = SearchSelect::new("choices", ["Alpha", "Beta"]);
    let mut cx = Context::new();
    for key in ['e', 't'] {
        search.handle(Event::char(key), &mut cx);
    }

    let surface = draw(&search, None);

    assert_eq!(surface.plain_text().lines().nth(1), Some("> Beta"));
    let theme = Theme::default();
    let cells = surface.rows()[1].cells();
    assert!(cells.len() > 5);
    assert_eq!(cells[2].style(), theme.style(Role::Selected));
    assert_eq!(cells[3].style(), theme.style(Role::Match));
    assert_eq!(cells[4].style(), theme.style(Role::Match));
    assert_eq!(cells[5].style(), theme.style(Role::Selected));
}

#[test]
fn explicit_continuation_lines_are_indented_under_the_marker() {
    let multi = MultiSelect::new("choices", ["first\n↳ second"]);

    assert_eq!(
        draw(&multi, None)
            .plain_text()
            .lines()
            .take(2)
            .collect::<Vec<_>>(),
        ["> [ ] first", "      ↳ second"]
    );
}

#[test]
fn form_cursor_follows_the_active_field() {
    let mut form = Form::new("form")
        .with_field(
            "name",
            TextInput::new("name")
                .with_prompt("Name: ")
                .with_value("Ada"),
        )
        .with_field(
            "note",
            TextInput::new("note")
                .with_prompt("Note: ")
                .with_value("hi"),
        );

    assert_eq!(
        draw(&form, None).cursor(),
        Some(Position { row: 1, col: 9 })
    );

    form.set_active_index(1);
    assert_eq!(
        draw(&form, None).cursor(),
        Some(Position { row: 3, col: 8 })
    );
}

#[test]
fn a_date_picker_marks_selected_today_and_outside_month_days() {
    let picker = DatePicker::new("when", date(1)).with_today(date(2));

    let surface = draw(&picker, None);

    let plain = surface.plain_text();
    let mut rows = plain.lines();
    assert_eq!(rows.next(), Some("July 2026"));
    assert_eq!(rows.next(), Some(" Mo  Tu  We  Th  Fr  Sa  Su"));
    assert_eq!(rows.next(), Some(".29 .30 > 1 * 2   3   4   5"));
    let theme = Theme::default();
    let week = surface.rows()[2].cells();
    assert!(week.len() > 12);
    assert_eq!(week[0].style(), theme.style(Role::Dim));
    assert_eq!(week[8].style(), theme.style(Role::Selected));
    assert_eq!(week[12].style(), theme.style(Role::Success));
}

#[test]
fn physical_rows_drive_page_navigation() {
    let select = Select::new("choices", [
        "one",
        "two\ncontinued",
        "three",
        "four",
        "five",
    ])
    .with_header("choose");
    let mut session = Session::new(select);
    session.handle(Event::Resize { cols: 40, rows: 5 });

    assert_eq!(lines(&session, 5), [
        "choose",
        "> one",
        "  two",
        "  continued",
        "up/down move | enter submit | esc cancel"
    ]);

    assert!(session.handle(page_key(Key::PageDown)).changed());

    assert_eq!(lines(&session, 5), [
        "choose",
        "> three",
        "  four",
        "  five",
        "up/down move | enter submit | esc cancel"
    ]);
}

#[test]
fn page_down_and_page_up_move_a_full_page_in_select() {
    let select = Select::new("choices", items(40)).with_header("choose");
    let mut session = Session::new(select);
    let first_row = |current: &Session| lines(current, 12)[1].clone();

    assert_eq!(first_row(&session), "> item-00");

    assert!(session.handle(page_key(Key::PageDown)).changed());
    assert_eq!(first_row(&session), "> item-09");

    assert!(session.handle(page_key(Key::PageDown)).changed());
    assert_eq!(first_row(&session), "> item-18");

    assert!(session.handle(page_key(Key::PageUp)).changed());
    assert_eq!(first_row(&session), "> item-09");
}

#[test]
fn page_down_moves_a_full_page_in_search_select() {
    let search = SearchSelect::new("choices", items(40)).with_prompt("search: ");
    let mut session = Session::new(search);

    assert_eq!(lines(&session, 12)[1], "> item-00");

    assert!(session.handle(page_key(Key::PageDown)).changed());
    assert_eq!(lines(&session, 12)[1], "> item-09");
}

#[test]
fn page_down_moves_a_full_page_in_review_list() {
    let review = ReviewList::new("choices", items(40)).with_header("review");
    let mut session = Session::new(review);

    assert_eq!(lines(&session, 12)[1], "> [*] item-00");

    assert!(session.handle(page_key(Key::PageDown)).changed());
    assert_eq!(lines(&session, 12)[1], "> [*] item-09");
}

#[test]
fn paging_without_a_rendered_frame_scrolls_by_the_page_size() {
    let select = Select::new("choices", items(40)).with_page_size(5);
    let mut session = Session::new(select);

    assert!(session.handle(page_key(Key::PageDown)).changed());

    let shown = lines(&session, 12);
    assert_eq!(shown.first().map(String::as_str), Some("  item-01"));
    assert!(shown.iter().any(|line| line == "> item-05"));
}

#[test]
fn page_keys_after_unrendered_moves_scroll_by_the_page_size() {
    let mut select = Select::new("choices", items(50)).with_page_size(5);
    let mut review = ReviewList::new("choices", items(50)).with_page_size(5);
    let mut cx = Context::new();
    drop(lines(&select, 12));
    drop(lines(&review, 12));
    for _ in 0..20 {
        select.handle(page_key(Key::Down), &mut cx);
        review.handle(page_key(Key::Down), &mut cx);
    }

    select.handle(page_key(Key::PageDown), &mut cx);
    review.handle(page_key(Key::PageDown), &mut cx);

    assert!(lines(&select, 12).iter().any(|line| line == "> item-25"));
    assert!(
        lines(&review, 12)
            .iter()
            .any(|line| line == "> [*] item-25")
    );
}

#[test]
fn a_form_shows_the_active_list_in_full() {
    let mut form = Form::new("form")
        .with_field("first", Select::new("a", ["one", "two", "three"]))
        .with_field("second", Select::new("b", ["four", "five", "six"]));

    let first = lines(&form, 9);

    assert!(first.iter().any(|line| line == "> one"));
    assert!(first.iter().any(|line| line == "  three"));
    assert!(!first.iter().any(|line| line.contains("four")));

    form.set_active_index(1);
    let second = lines(&form, 9);

    assert!(second.iter().any(|line| line == "> four"));
    assert!(second.iter().any(|line| line == "  six"));
    assert!(!second.iter().any(|line| line.contains("one")));
}

fn text_form(count: usize) -> Form {
    let mut form = Form::new("form");
    for index in 0..count {
        form.push_field(
            format!("field{index}"),
            TextInput::new(format!("f{index}"))
                .with_prompt(format!("f{index}> "))
                .with_value("v"),
        );
    }
    form
}

#[test]
fn a_form_scrolls_to_keep_the_active_field_in_view() {
    for height in 1..=20 {
        let mut form = text_form(8);
        for active in (0..8).chain((0..7).rev()) {
            form.set_active_index(active);
            let surface = draw(&form, Some(height));
            let shown: Vec<_> = surface.plain_text().lines().map(str::to_owned).collect();
            let context = format!("field {active} at height {height}");

            assert!(shown.len() <= height, "{context}");
            let cursor = surface.cursor().expect(&context);
            assert!(
                shown[cursor.row].starts_with(&format!("f{active}> ")),
                "{context}"
            );
            if height >= 2 {
                assert_eq!(
                    shown[cursor.row - 1],
                    format!("> field{active}"),
                    "{context}"
                );
            }
            if height >= 3 {
                let help = shown.last().expect(&context);
                assert!(help.starts_with("tab next | shift-tab"), "{context}");
            }
        }
    }
}

#[test]
fn a_list_field_in_a_form_keeps_rows_at_small_heights() {
    for height in 2..=24 {
        let mut form = Form::new("form")
            .with_field("name", TextInput::new("name").with_prompt("name> "))
            .with_field("pick", Select::new("pick", items(30)))
            .with_field("note", TextInput::new("note").with_prompt("note> "));
        form.set_active_index(1);
        let shown = lines(&form, height);
        let context = format!("height {height}");

        assert!(shown.len() <= height, "{context}");
        assert!(shown.iter().any(|line| line == "> item-00"), "{context}");
        assert_eq!(
            shown.iter().any(|line| line == "> pick"),
            height >= 3,
            "{context}"
        );
        assert_eq!(
            shown.last().map(String::as_str) == Some("tab next | shift-tab previous"),
            height >= 4,
            "{context}"
        );
    }
}

#[test]
fn a_row_event_picks_a_row_in_every_list() {
    let mut cx = Context::new();
    let mut select = Select::new("select", items(5));
    let mut multi = MultiSelect::new("multi", items(5));
    let mut search = SearchSelect::new("search", items(5));
    let mut review = ReviewList::new("review", items(5));

    for row in [3, 0] {
        assert_eq!(select.handle(Event::Row(row), &mut cx), Reaction::Changed);
        assert_eq!(select.selected_index(), Some(row));
        assert_eq!(multi.handle(Event::Row(row), &mut cx), Reaction::Changed);
        assert_eq!(multi.selected_index(), Some(row));
        assert_eq!(search.handle(Event::Row(row), &mut cx), Reaction::Changed);
        assert_eq!(search.selected_match_index(), Some(row));
        assert_eq!(review.handle(Event::Row(row), &mut cx), Reaction::Changed);
        assert_eq!(review.selected_index(), Some(row));
    }
    assert_eq!(select.handle(Event::Row(5), &mut cx), Reaction::Ignored);
    assert_eq!(multi.handle(Event::Row(5), &mut cx), Reaction::Ignored);
    assert_eq!(search.handle(Event::Row(5), &mut cx), Reaction::Ignored);
    assert_eq!(review.handle(Event::Row(5), &mut cx), Reaction::Ignored);
}

#[test]
fn only_a_handled_event_forgets_the_rendered_page_layout() {
    for (event, landed) in [
        (Event::Row(20), 40),
        (Event::Tick, 9),
        (Event::Paste("x".to_owned()), 9),
        (Event::Resize { cols: 40, rows: 10 }, 9),
    ] {
        let mut select = Select::new("choices", items(50)).with_page_size(20);
        let mut cx = Context::new();
        drop(lines(&select, 10));
        select.handle(event, &mut cx);
        select.handle(page_key(Key::PageDown), &mut cx);

        assert_eq!(select.selected_index(), Some(landed));
    }
}

#[test]
fn a_chord_on_a_plain_key_is_ignored_by_every_widget() {
    let widgets: Vec<Box<dyn bang_core::Widget>> = vec![
        Box::new(Select::new("select", items(3))),
        Box::new(MultiSelect::new("multi", items(3))),
        Box::new(SearchSelect::new("search", items(3))),
        Box::new(ReviewList::new("review", items(3))),
        Box::new(TextInput::new("text")),
        Box::new(DatePicker::new("when", date(1))),
        Box::new(Form::new("form").with_field("text", TextInput::new("inner"))),
    ];
    let mut cx = Context::new();

    for mut widget in widgets {
        for modifiers in [Modifiers::CONTROL, Modifiers::ALT] {
            for key in [
                Key::Enter,
                Key::Esc,
                Key::Tab,
                Key::Char(' '),
                Key::Up,
                Key::Down,
                Key::Left,
                Key::Right,
            ] {
                let event = Event::Key(KeyEvent::with_modifiers(key.clone(), modifiers));
                assert_eq!(
                    widget.handle(event, &mut cx),
                    Reaction::Ignored,
                    "{key:?} with {modifiers:?} on {:?}",
                    widget.id()
                );
            }
        }
    }
}

#[test]
fn page_keys_clamp_at_both_ends_of_a_list() {
    let mut select = Select::new("choices", items(50)).with_page_size(20);
    let mut cx = Context::new();
    drop(lines(&select, 10));

    let mut last = 0;
    for _ in 0..12 {
        select.handle(page_key(Key::PageDown), &mut cx);
        drop(lines(&select, 10));
        let selected = select.selected_index().expect("a selection");
        assert!(
            selected >= last,
            "page down moved back to {selected} from {last}"
        );
        last = selected;
    }
    assert_eq!(last, 49);

    let mut descending = 49;
    for _ in 0..12 {
        select.handle(page_key(Key::PageUp), &mut cx);
        drop(lines(&select, 10));
        let selected = select.selected_index().expect("a selection");
        assert!(
            selected <= descending,
            "page up moved forward to {selected} from {descending}"
        );
        descending = selected;
    }
    assert_eq!(descending, 0);
}

#[test]
fn paging_past_the_end_without_a_frame_does_not_wrap() {
    let mut select = Select::new("choices", items(8)).with_page_size(5);
    let mut cx = Context::new();

    select.handle(page_key(Key::PageDown), &mut cx);
    select.handle(page_key(Key::PageDown), &mut cx);
    select.handle(page_key(Key::PageDown), &mut cx);

    assert_eq!(select.selected_index(), Some(7));
}

#[test]
fn a_list_fills_a_terminal_that_grows_in_one_frame() {
    let mut select = Select::new("choices", items(100)).with_page_size(20);
    let mut cx = Context::new();
    assert_eq!(lines(&select, 6).len(), 6);

    select.handle(Event::Resize { cols: 40, rows: 30 }, &mut cx);

    assert_eq!(lines(&select, 30).len(), 21);
}
