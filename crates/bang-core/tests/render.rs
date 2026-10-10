// SPDX-License-Identifier: EUPL-1.2

use bang_core::{
    Context,
    Date,
    Event,
    Key,
    KeyEvent,
    Modifiers,
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

const fn date(day: u8) -> Date {
    Date::new(2026, 7, day).expect("test date is valid")
}

#[test]
fn a_list_keeps_plain_content_and_semantic_styles() {
    let multi = MultiSelect::new("choices", ["Alpha", "Beta"])
        .with_header("Choose")
        .with_checked_indices([0]);

    let surface = draw(&multi, None);
    assert_eq!(
        surface.plain_text(),
        "Choose\n> [x] Alpha\n  [ ] Beta\nenter submit | esc cancel"
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
    assert_eq!(rows.next(), Some("Mo Tu We Th Fr Sa Su"));
    assert_eq!(rows.next(), Some(".29 .30 > 1 * 2   3   4   5"));
    let theme = Theme::default();
    let week = surface.rows()[2].cells();
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
        "enter submit | esc cancel"
    ]);

    assert!(session.handle(page_key(Key::PageDown)).changed());

    assert_eq!(lines(&session, 5), [
        "choose",
        "> three",
        "  four",
        "  five",
        "enter submit | esc cancel"
    ]);
}

#[test]
fn page_down_and_page_up_move_a_full_page_in_select() {
    let select = Select::new("choices", items(40)).with_header("choose");
    let mut session = Session::new(select);
    let first_row = |session: &Session| lines(session, 12)[1].clone();

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
fn a_form_shares_height_between_list_fields() {
    let form = Form::new("form")
        .with_field("first", Select::new("a", ["one", "two", "three"]))
        .with_field("second", Select::new("b", ["four", "five", "six"]));

    let shown = lines(&form, 9);

    assert!(shown.iter().any(|line| line == "> one"));
    assert!(shown.iter().any(|line| line == "  two"));
    assert!(!shown.iter().any(|line| line.contains("three")));
    assert!(shown.iter().any(|line| line == "> four"));
    assert!(shown.iter().any(|line| line == "  five"));
    assert!(!shown.iter().any(|line| line.contains("six")));
}

#[test]
fn page_keys_clamp_at_both_ends_of_a_list() {
    let mut select = Select::new("choices", items(50)).with_page_size(20);
    let mut cx = Context::new();
    let _ = lines(&select, 10);

    let mut last = 0;
    for _ in 0..12 {
        select.handle(page_key(Key::PageDown), &mut cx);
        let _ = lines(&select, 10);
        let selected = select.selected_index().expect("a selection");
        assert!(
            selected >= last,
            "page down moved back to {selected} from {last}"
        );
        last = selected;
    }
    assert_eq!(last, 49);

    let mut last = 49;
    for _ in 0..12 {
        select.handle(page_key(Key::PageUp), &mut cx);
        let _ = lines(&select, 10);
        let selected = select.selected_index().expect("a selection");
        assert!(
            selected <= last,
            "page up moved forward to {selected} from {last}"
        );
        last = selected;
    }
    assert_eq!(last, 0);
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

#[test]
fn a_focus_request_does_not_swallow_a_submit() {
    use bang_core::{
        FocusTarget,
        Reaction,
        SessionReaction,
        SessionStatus,
        WidgetId,
    };
    use screw::Widget as ScrewWidget;

    struct FocusThenSubmit;

    impl ScrewWidget for FocusThenSubmit {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl bang_core::Widget for FocusThenSubmit {
        fn id(&self) -> WidgetId {
            WidgetId::borrowed("both")
        }

        fn handle(&mut self, _event: Event, cx: &mut Context) -> Reaction {
            cx.request_focus(FocusTarget::Next);
            Reaction::Submit(bang_core::Value::from("done"))
        }
    }

    let mut session = Session::new(FocusThenSubmit);

    assert_eq!(
        session.handle(Event::char('x')),
        SessionReaction::Submit(bang_core::Value::from("done"))
    );
    assert!(matches!(session.status(), SessionStatus::Submitted(_)));
}
