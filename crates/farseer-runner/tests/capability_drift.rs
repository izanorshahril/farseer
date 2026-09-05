//! Does each runner still do what farseer's tables say it does?
//!
//! Every capability table in this repo - `HARNESS.md`'s matrix,
//! `pi::loads_skills_by_path`, `pi::takes_tool_allowlist`,
//! `farseer_manager::deny_discovered_tools` - was written from a live probe on
//! one machine on one day. `10 runner inventory`'s rule is **observed, never
//! advertised**, and the cost of that rule is that an observation expires: the
//! binaries update on their own schedule and nothing re-checks.
//!
//! This is the re-check. It is the only test suite in the repo whose failure
//! means *the world changed*, not *the code is wrong* - so a failure here is
//! read as "go and re-probe", never as "fix the assertion".
//!
//! **These cost nothing.** They read `--help` and exit; no model is invoked and
//! no subscription is spent, which is what separates them from the `#[ignore]`
//! runs in `farseer-manager` that do both. They are still ignored by default
//! because they need the binaries installed, and a machine without `omp` should
//! not have a red suite.
//!
//! ```text
//! cargo test -p farseer-runner --test capability_drift -- --ignored
//! ```
//!
//! A missing binary **skips** rather than fails: farseer holds its inventory as
//! a menu an operator picks from (`13 harness build kit`), and not owning a
//! runner is a choice rather than a fault.

use std::process::Command;

/// The binary's own help text, or `None` if it is not installed here.
fn help(exe: &str, args: &[&str]) -> Option<String> {
    let path = farseer_runner::resolve::resolve(exe)?;
    let out = Command::new(path).args(args).arg("--help").output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    // Several of these print help on stderr, and which one is not a fact worth
    // depending on.
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(text)
}

#[track_caller]
fn assert_flag(text: &str, flag: &str, expected: bool, runner: &str, table: &str) {
    let found = text.contains(flag);
    assert_eq!(
        found,
        expected,
        "{runner} {} `{flag}`, and {table} says it {}. \
         Re-probe the runner and update the table - do not edit this assertion \
         to match. See `HARNESS.md` section 6a.",
        if found { "offers" } else { "no longer offers" },
        if expected { "does" } else { "does not" },
    );
}

/// `32 harness capability floor` and `36 tool grant enforcement`: pi loads a
/// skill by path and omp cannot, which is why `loads_skills_by_path` has one arm.
#[test]
#[ignore = "reads installed runners' help output; costs nothing, needs them present"]
fn only_pi_still_loads_a_skill_by_path() {
    if let Some(text) = help("pi", &[]) {
        assert_flag(&text, "--skill ", true, "pi", "`pi::loads_skills_by_path`");
        assert!(
            farseer_runner::pi::loads_skills_by_path("pi"),
            "the table and the binary must agree"
        );
    }
    if let Some(text) = help("omp", &[]) {
        // The distinction the table exists for: `--skills` is a **glob filter**
        // over what omp discovered, not a loader, so there is no argv that hands
        // it a directory. If omp ever grows `--skill`, this fails and the table
        // is the thing to change.
        assert_flag(
            &text,
            "--skill=",
            false,
            "omp",
            "`pi::loads_skills_by_path`",
        );
        assert!(!farseer_runner::pi::loads_skills_by_path("omp"));
    }
}

/// `32`: omp has no `ask_question` and no flag to deny one, which is what killed
/// its first launch through farseer.
#[test]
#[ignore = "reads installed runners' help output; costs nothing, needs them present"]
fn pi_still_denies_the_tool_that_waits_for_a_person_and_omp_still_cannot() {
    if let Some(text) = help("pi", &[]) {
        assert_flag(&text, "--exclude-tools", true, "pi", "`pi::build_args`");
    }
    if let Some(text) = help("omp", &[]) {
        assert_flag(&text, "--exclude-tools", false, "omp", "`pi::build_args`");
    }
}

/// `36 tool grant enforcement`: both take an allowlist, which is what makes
/// `ToolLevel` enforceable at all.
#[test]
#[ignore = "reads installed runners' help output; costs nothing, needs them present"]
fn the_two_runners_that_take_a_tool_allowlist_still_take_one() {
    for runner in ["pi", "omp"] {
        if let Some(text) = help(runner, &[]) {
            assert_flag(&text, "--tools", true, runner, "`pi::takes_tool_allowlist`");
            assert!(farseer_runner::pi::takes_tool_allowlist(runner));
        }
    }
}

/// `37 inherited tool environment`: opencode can be told to ignore the
/// operator's plugins and goose cannot. The **negative** is the load-bearing
/// half - if goose grows a denial flag, farseer should start passing it.
#[test]
#[ignore = "reads installed runners' help output; costs nothing, needs them present"]
fn opencode_still_runs_pure_and_goose_still_offers_nothing_that_subtracts() {
    if let Some(text) = help("opencode", &["acp"]) {
        assert_flag(&text, "--pure", true, "opencode", "`deny_discovered_tools`");
    }
    if let Some(text) = help("goose", &["acp"]) {
        assert!(
            text.contains("--with-builtin"),
            "goose acp no longer offers `--with-builtin`; re-probe `37 inherited tool environment`"
        );
        for absent in ["--pure", "--no-extensions", "--without-builtin"] {
            assert!(
                !text.contains(absent),
                "goose acp now offers `{absent}` - `37 inherited tool environment` \
                 recorded that it had no way to subtract, and farseer should start \
                 passing this. Update `deny_discovered_tools`."
            );
        }
    }
}

/// A runner that resolves to a Windows `.CMD` cannot be handed a newline.
///
/// This is the only test here that watches something other than a flag, and it
/// exists because a flag was never what broke. On 2026-08-30 pi shipped an
/// update whose shims no longer include an `.exe`, so farseer resolved `pi.CMD`.
/// Rust refuses to spawn a batch file with an argument containing a newline,
/// answering `batch file arguments are invalid` and naming nothing, so every
/// manager run on the machine stopped working overnight without a line of
/// farseer changing.
///
/// So the fact worth watching is **what kind of file each runner resolves to**.
/// A runner on a batch shim may only be passed single-line arguments, which is
/// why `pi::reads_prompt_from_file` exists and why the manager prompt is handed
/// over as a path.
#[test]
#[ignore = "reads installed runners' help output; costs nothing, needs them present"]
fn a_runner_on_a_batch_shim_still_reads_its_prompt_from_a_file() {
    for runner in ["pi", "omp"] {
        let Some(path) = farseer_runner::resolve::resolve(runner) else {
            continue;
        };
        let batch = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
        if !batch {
            continue;
        }
        assert!(
            farseer_runner::pi::reads_prompt_from_file(runner),
            "{runner} now resolves to {}, a batch file, which cannot take a              multi-line argument - and farseer has no file route for its prompt.              Re-probe `--append-system-prompt` and update `reads_prompt_from_file`.",
            path.display()
        );
        if let Some(text) = help(runner, &[]) {
            assert!(
                text.contains("file contents"),
                "{runner} no longer says `--append-system-prompt` takes file contents,                  and it is on a batch shim - farseer's prompt has nowhere to go"
            );
        }
    }
}

/// `30 codex app server`: the app-server generates its own protocol schema,
/// which is the one runner surface farseer does not have to transcribe. If this
/// subcommand disappears, every payload shape goes back to being a guess.
#[test]
#[ignore = "reads installed runners' help output; costs nothing, needs them present"]
fn codex_still_generates_its_own_protocol_schema() {
    if let Some(text) = help("codex", &["app-server"]) {
        assert!(
            text.contains("generate-json-schema"),
            "`codex app-server generate-json-schema` is gone; `30 codex app server` \
             relied on it for exact payload shapes"
        );
    }
}
