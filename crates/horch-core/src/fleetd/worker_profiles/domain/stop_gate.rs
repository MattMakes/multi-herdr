//! The stop gate of a worker harness: the stop hook rules (ticket 14).
//!
//! FM-6, the inside end: a push attempt does not end its turn while both
//! `result.json` and a question file are missing. The Stop hook of each
//! adapter runs [`hook_command`], and `horch result --check` applies
//! [`may_end`].

use std::path::Path;

/// The line of the stop gate while the attempt has no result and no
/// question.
pub const RESULT_FIRST: &str = "run `horch result` first";

/// What the outbox of an attempt holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutboxFacts {
    /// `outbox/result.json` is a regular file.
    pub result_json: bool,
    /// `outbox/questions/` holds a question file.
    pub question_file: bool,
}

/// The ruling of the stop gate on 1 turn end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The turn ends.
    End,
    /// The agent works on: [`RESULT_FIRST`].
    KeepWorking,
}

/// The turn may end with a result or a question only.
pub fn may_end(outbox: &OutboxFacts) -> Decision {
    if outbox.result_json || outbox.question_file {
        Decision::End
    } else {
        Decision::KeepWorking
    }
}

/// The shell command of a Stop hook: `horch result --check` with the
/// absolute path of the `horch` that the launch knows, quoted for a POSIX
/// shell.
pub fn hook_command(horch_exe: &Path) -> String {
    let exe = horch_exe.to_string_lossy().replace('\'', r"'\''");
    format!("'{exe}' result --check")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_may_end_with_a_result_or_a_question_only() {
        let facts = |result_json, question_file| OutboxFacts {
            result_json,
            question_file,
        };
        assert_eq!(may_end(&facts(false, false)), Decision::KeepWorking);
        assert_eq!(may_end(&facts(true, false)), Decision::End);
        assert_eq!(may_end(&facts(false, true)), Decision::End);
        assert_eq!(may_end(&facts(true, true)), Decision::End);
    }

    #[test]
    fn hook_command_quotes_the_horch_path() {
        assert_eq!(
            hook_command(Path::new("/opt/my bin/horch")),
            "'/opt/my bin/horch' result --check"
        );
        assert_eq!(
            hook_command(Path::new("/a'b/horch")),
            r"'/a'\''b/horch' result --check"
        );
    }
}
