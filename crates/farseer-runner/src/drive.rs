//! Reads a [`SupervisedProcess`]'s stdout line by line and hands each line's
//! parse result to a caller-supplied sink.
//!
//! **Every line reaches the sink, parse failure or not.** `05 run state model`'s activity
//! signal is "any bytes", so a line this crate fails to parse is still
//! evidence the process is alive; silently dropping it would throw that
//! evidence away. The sink decides what a parse failure means for its own
//! record - most callers should still mark activity and move on, exactly as
//! for an unrecognised-but-valid line.
//!
//! `parse` is a parameter, not a hardcoded call, because more than one
//! runner speaks stream-json now: [`crate::claude_code::parse_line`] and
//! [`crate::codex::parse_line`] both fit the same shape.

use crate::claude_code::{ParseError, RunnerSignal};
use crate::spawn::SupervisedProcess;

/// Drains `proc` until it closes stdout (normally: exits), calling `on_line`
/// once per line with that line's parse result. Returns once the process has
/// gone quiet on stdout - it does not itself wait for exit or read stderr.
pub fn drive<F>(
    proc: &mut SupervisedProcess,
    parse: impl Fn(&str) -> Result<Vec<RunnerSignal>, ParseError>,
    mut on_line: F,
) -> std::io::Result<()>
where
    F: FnMut(Result<Vec<RunnerSignal>, ParseError>),
{
    while let Some(line) = proc.read_line()? {
        on_line(parse(&line));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn::StdinMode;
    use farseer_core::run::Outcome;
    use std::path::Path;

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_stdout_line_from_a_real_child_reaches_the_sink_in_order() {
        // Piped through a file with `type` rather than `echo`, so cmd's own
        // quote handling never touches the embedded JSON.
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir.path().join("lines.txt");
        std::fs::write(
            &fixture,
            concat!(
                r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1,"rateLimitType":"five_hour"}}"#,
                "\r\n",
                "not json",
                "\r\n",
                r#"{"type":"result","subtype":"success","total_cost_usd":0.01}"#,
                "\r\n",
            ),
        )
        .unwrap();

        let mut proc = SupervisedProcess::spawn(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &cmd(&["/c", "type", fixture.to_str().unwrap()]),
            &std::env::current_dir().unwrap(),
            &[],
            StdinMode::Live,
        )
        .unwrap();

        let mut lines_seen = 0;
        let mut parse_errors = 0;
        let mut finished_outcome = None;
        drive(&mut proc, crate::claude_code::parse_line, |result| {
            lines_seen += 1;
            match result {
                Ok(signals) => {
                    for signal in signals {
                        if let RunnerSignal::Finished(f) = signal {
                            finished_outcome = Some(f.outcome);
                        }
                    }
                }
                Err(_) => parse_errors += 1,
            }
        })
        .unwrap();

        assert_eq!(
            lines_seen, 3,
            "all three lines reach the sink, valid or not"
        );
        assert_eq!(
            parse_errors, 1,
            "the malformed line surfaces as a parse error, not silence"
        );
        assert_eq!(finished_outcome, Some(Outcome::Ok));
    }
}
