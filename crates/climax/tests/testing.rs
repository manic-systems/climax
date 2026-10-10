// SPDX-License-Identifier: EUPL-1.2

#![cfg(all(
    feature = "derive",
    feature = "interactive",
    feature = "structured"
))]

use climax::{
    bang::Date,
    prelude::*,
    testing::{self, Script},
};

/// ship a build
#[derive(Clone, Copy, Parse)]
struct Args {
    /// print the result as JSON
    #[pound(long)]
    json: bool,
    /// fail after the prompts
    #[pound(long)]
    fail: bool,
}

/// pick a tool
#[expect(dead_code, reason = "parsed only to reach the subcommand error")]
#[derive(Parse)]
struct Tools {
    #[pound(subcommand)]
    command: Tool,
}

#[derive(Parse)]
#[expect(dead_code, reason = "parsed only to reach the subcommand error")]
enum Tool {
    /// list tools
    List {
        /// show everything
        #[pound(long)]
        all: bool,
    },
}

#[climax::serde(Serialize)]
struct Shipped {
    region: String,
    target: String,
}

fn ship(mut cx: Context, args: Args) -> climax::Result<()> {
    if args.json {
        cx.set_output_format(Format::Json);
    }
    let region = cx
        .select("Region")
        .choice("eu", "eu")
        .choice("us", "us")
        .interact()?
        .or_cancel()?;
    let target = cx.text("Target").interact()?.or_cancel()?;
    if !cx.confirm("Ship it?").interact()?.or_cancel()? {
        return Err(Error::message("declined").with_exit_code(4));
    }
    if args.fail {
        return Err(Error::message("registry refused the upload").with_exit_code(3));
    }
    let shipped = Shipped {
        region: region.to_owned(),
        target,
    };
    cx.output()
        .result(&shipped)
        .text(|shipped| format!("shipped {} to {}", shipped.target, shipped.region))
        .emit()
}

fn ask(cx: &Context) -> climax::Result<()> {
    cx.confirm("Go?").interact()?.or_cancel()?;
    Ok(())
}

fn full_script() -> Script {
    Script::new().select_nth(1).text("prod").confirm(true)
}

#[test]
fn a_prompt_flow_prints_its_result() {
    let outcome = testing::run(Vec::<&str>::new(), full_script(), ship);
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout, "shipped prod to us\n");
    assert_eq!(outcome.stderr, "");
    assert!(outcome.error.is_none());
}

#[test]
fn json_output_is_captured_as_one_document() {
    let outcome = testing::run(["--json"], full_script(), ship);
    assert_eq!(outcome.exit_code, 0);
    let value: serde_json::Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(value["region"], "us");
    assert_eq!(value["target"], "prod");
}

#[test]
fn an_error_maps_to_its_exit_code_and_reaches_stderr() {
    let outcome = testing::run(["--fail"], full_script(), ship);
    assert_eq!(outcome.exit_code, 3);
    assert_eq!(outcome.stdout, "");
    assert_eq!(outcome.stderr, "error: registry refused the upload\n");
    assert_eq!(outcome.error.unwrap().kind(), ErrorKind::Message);
}

#[test]
fn declining_the_confirm_exits_with_the_applications_code() {
    let outcome = testing::run(
        Vec::<&str>::new(),
        Script::new().select_nth(0).text("dev").confirm(false),
        ship,
    );
    assert_eq!(outcome.exit_code, 4);
    assert_eq!(outcome.stdout, "");
}

#[test]
fn leaving_a_prompt_cancels_with_exit_130() {
    let outcome = testing::run(Vec::<&str>::new(), Script::new().esc(), ship);
    assert_eq!(outcome.exit_code, 130);
    assert_eq!(outcome.stdout, "");
    assert_eq!(outcome.stderr, "");
    assert_eq!(outcome.error.unwrap().kind(), ErrorKind::Cancelled);
}

#[test]
fn a_parse_failure_exits_2_and_help_exits_0() {
    let outcome = testing::run(["--wat"], Script::new(), ship);
    assert_eq!(outcome.exit_code, 2);
    assert_eq!(outcome.error.unwrap().kind(), ErrorKind::Parse);
    assert!(outcome.stderr.starts_with("error"), "got {:?}", outcome.stderr);

    let outcome = testing::run(["--help"], Script::new(), ship);
    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.contains("ship a build"), "got {:?}", outcome.stdout);
}

#[test]
fn run_with_needs_no_arguments() {
    let outcome = testing::run_with(Script::new().confirm(true), |cx| {
        ask(&cx)
    });
    assert_eq!(outcome.exit_code, 0);
}

#[test]
#[should_panic(expected = "testing script was not fully consumed")]
fn leftover_script_input_fails_the_run() {
    let _ = testing::run(
        Vec::<&str>::new(),
        full_script().text("never asked"),
        ship,
    );
}

#[test]
#[should_panic(expected = "testing script was not fully consumed")]
fn an_unread_tail_of_a_prompt_answer_fails_the_run() {
    let _ = testing::run_with(Script::new().keys([
        climax::bang::advanced::Key::Char('y'),
        climax::bang::advanced::Key::Enter,
    ]), |cx| {
        ask(&cx)
    });
}

#[test]
fn a_prompt_past_the_end_of_the_script_is_an_input_ended_error() {
    let outcome = testing::run_with(Script::new(), |cx| {
        ask(&cx)
    });
    assert_eq!(outcome.exit_code, 1);
    assert_eq!(outcome.error.unwrap().kind(), ErrorKind::InputEnded);
}

#[test]
fn multi_select_nth_toggles_rows_in_any_order() {
    let outcome = testing::run_with(Script::new().multi_select_nth([2, 0]), |cx| {
        let picked = cx
            .multi_select("Tools")
            .choice("a", "a")
            .choice("b", "b")
            .choice("c", "c")
            .interact()?
            .or_cancel()?;
        cx.diagnostic().notice(format!("{picked:?}"))
    });
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stderr, "[\"a\", \"c\"]\n");
}

#[test]
fn a_missing_subcommand_exits_2_on_stderr_with_nothing_on_stdout() {
    let outcome = testing::run(Vec::<&str>::new(), Script::new(), |_cx, _tools: Tools| Ok(()));
    assert_eq!(outcome.exit_code, 2);
    assert_eq!(outcome.stdout, "");
    assert!(
        outcome.stderr.starts_with("error: a subcommand is required"),
        "got {:?}",
        outcome.stderr
    );
    assert!(outcome.stderr.contains("list"), "got {:?}", outcome.stderr);
    assert_eq!(outcome.error.unwrap().kind(), ErrorKind::Parse);
}

#[test]
fn select_nth_is_absolute_over_a_preselection() {
    let outcome = testing::run_with(Script::new().select_nth(0), |cx| {
        let picked = cx
            .select("Region")
            .choice("eu", "eu")
            .choice("us", "us")
            .choice("ap", "ap")
            .selected(2)
            .interact()?
            .or_cancel()?;
        cx.diagnostic().notice(picked.to_string())
    });
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stderr, "eu\n");
}

#[test]
fn multi_select_nth_replaces_the_preselection() {
    let outcome = testing::run_with(Script::new().multi_select_nth([2]), |cx| {
        let picked = cx
            .multi_select("Tools")
            .choice("a", "a")
            .choice("b", "b")
            .choice("c", "c")
            .checked(0)
            .interact()?
            .or_cancel()?;
        cx.diagnostic().notice(format!("{picked:?}"))
    });
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stderr, "[\"c\"]\n");
}

fn date_flow(from: Date, to: Date) -> Date {
    let picked = std::cell::Cell::new(None);
    let outcome = testing::run_with(Script::new().date(from, to), |cx| {
        picked.set(Some(cx.date("When").default(from).interact()?.or_cancel()?));
        Ok(())
    });
    assert_eq!(outcome.exit_code, 0);
    picked.get().unwrap()
}

#[test]
fn date_moves_forward_across_years_and_clamped_months() {
    let from = Date::new(2026, 10, 31).unwrap();
    let to = Date::new(2028, 2, 29).unwrap();
    assert_eq!(date_flow(from, to), to);
}

#[test]
fn date_moves_backward_and_within_a_month() {
    let from = Date::new(2026, 3, 31).unwrap();
    let to = Date::new(2024, 12, 5).unwrap();
    assert_eq!(date_flow(from, to), to);
    let to = Date::new(2026, 3, 2).unwrap();
    assert_eq!(date_flow(from, to), to);
}

#[test]
fn text_attempts_erase_a_rejected_input_before_retyping() {
    let script = Script::new().text_attempts(["ab", "cdef", "ghi"]);
    let outcome = testing::run_with(script, |cx| {
        let value = cx
            .text("Name")
            .validator(|text| {
                if text == "ghi" {
                    Ok(())
                } else {
                    Err(format!("{text} is taken"))
                }
            })
            .interact()?
            .or_cancel()?;
        cx.diagnostic().notice(value)
    });
    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.ends_with("ghi\n"), "got {:?}", outcome.stderr);
}
