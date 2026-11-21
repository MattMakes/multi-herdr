use std::path::Path;

pub const RESULT_FIRST: &str = "run `horch result` first";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutboxFacts {
    pub result_json: bool,
    pub question_file: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    End,
    KeepWorking,
}

pub fn may_end(outbox: &OutboxFacts) -> Decision {
    if outbox.result_json || outbox.question_file {
        Decision::End
    } else {
        Decision::KeepWorking
    }
}

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
