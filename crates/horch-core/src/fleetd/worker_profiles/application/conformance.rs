//! Run the conformance suite of an adapter through a port and record each
//! verdict. fleetd-p7 tickets 23 and 24 fill it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fleetd::worker_profiles::domain::conformance::{
    judge, Check, CheckResult, ConformanceReport, Observation, ProbeReport, Verdict, CHECKS,
};

/// The real launch of 1 check: its fixed prompt in a fresh throwaway task
/// folder, through the launcher and the wrapper.
pub trait ConformanceLaunch {
    /// The observation of the launch of `check`. An error is a launch that
    /// did not start, with its reason.
    fn launch(&mut self, check: Check) -> Result<Observation, String>;
}

/// The CLI under test: its name, its version text and the hash of its
/// adapter (fleetd-p7 ticket 25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteSubject {
    pub cli: String,
    pub version: String,
    pub adapter_hash: String,
}

/// The verdict of 1 check: its launch, then its rule. A launch that did not
/// start fails the check with its reason.
pub fn run_check(port: &mut dyn ConformanceLaunch, check: Check) -> CheckResult {
    match port.launch(check) {
        Ok(observation) => judge(check, &observation),
        Err(why) => CheckResult::fail(check, format!("the launch did not start: {why}")),
    }
}

/// Run each check of the suite of 1 exec CLI, in the order of the design,
/// and give the report of `conformance.json`.
pub fn run_suite(subject: SuiteSubject, port: &mut dyn ConformanceLaunch) -> ConformanceReport {
    let checks = CHECKS
        .into_iter()
        .filter(|c| c.in_exec_suite())
        .map(|c| run_check(port, c))
        .collect();
    ConformanceReport {
        cli: subject.cli,
        version: subject.version,
        adapter_hash: subject.adapter_hash,
        checks,
    }
}

/// The form of `conformance.json`.
#[derive(Serialize, Deserialize)]
struct ReportJson {
    cli: String,
    version: String,
    adapter_hash: String,
    checks: Vec<CheckJson>,
}

/// 1 check in `conformance.json`: its name, `pass` or `fail`, and 1 line of
/// evidence.
#[derive(Serialize, Deserialize)]
struct CheckJson {
    name: String,
    verdict: String,
    evidence: String,
}

/// The text of `conformance.json` for `report`.
pub fn conformance_json(report: &ConformanceReport) -> String {
    let json = ReportJson {
        cli: report.cli.clone(),
        version: report.version.clone(),
        adapter_hash: report.adapter_hash.clone(),
        checks: report
            .checks
            .iter()
            .map(|r| CheckJson {
                name: r.check.name().to_owned(),
                verdict: r.verdict.word().to_owned(),
                evidence: r.evidence.clone(),
            })
            .collect(),
    };
    let mut text = serde_json::to_string_pretty(&json).expect("a conformance report is plain data");
    text.push('\n');
    text
}

/// The report of the text of a `conformance.json`.
pub fn read_conformance_json(text: &str) -> Result<ConformanceReport, String> {
    let json: ReportJson = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let checks = json
        .checks
        .into_iter()
        .map(|c| {
            let check = Check::of_name(&c.name)
                .ok_or_else(|| format!("no conformance check {:?}", c.name))?;
            let verdict = Verdict::of_word(&c.verdict)
                .ok_or_else(|| format!("no verdict {:?}", c.verdict))?;
            Ok(CheckResult {
                check,
                verdict,
                evidence: c.evidence,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(ConformanceReport {
        cli: json.cli,
        version: json.version,
        adapter_hash: json.adapter_hash,
        checks,
    })
}

/// The report of the agent in its last message `text`: the JSON object from
/// its first `{` to its last `}`. `None` when there is none.
pub fn read_probe_report(text: &str) -> Option<ProbeReport> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    if !v.is_object() {
        return None;
    }
    let names = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let text_of = |v: &Value| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    Some(ProbeReport {
        skills: names("skills"),
        rules: names("rules"),
        mcp_servers: names("mcp_servers"),
        compaction: text_of(&v["compaction"]),
        result_write: text_of(&v["result_write"]),
        write_outside: text_of(&v["operator_boundary"]["write_outside"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_read_from_a_message_with_text_around_it() {
        let text = "Here: {\"skills\":[\"a\"],\"compaction\":256000,\
                    \"operator_boundary\":{\"write_outside\":\"refused: no\"}} done";
        let r = read_probe_report(text).unwrap();
        assert_eq!(r.skills, ["a"]);
        assert_eq!(r.compaction.as_deref(), Some("256000"));
        assert_eq!(r.write_outside.as_deref(), Some("refused: no"));
        assert_eq!(read_probe_report("no report"), None);
    }

    #[test]
    fn conformance_json_reads_back() {
        let report = ConformanceReport {
            cli: "claude".into(),
            version: "2.1.284 (Claude Code)".into(),
            adapter_hash: "h".into(),
            checks: vec![
                CheckResult::pass(Check::LaunchHeadless, "first event, exit 0"),
                CheckResult::fail(Check::StopGate, "no repair"),
            ],
        };
        let text = conformance_json(&report);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["checks"][1]["name"], "Stop gate");
        assert_eq!(v["checks"][1]["verdict"], "fail");
        assert_eq!(read_conformance_json(&text).unwrap(), report);
    }
}
