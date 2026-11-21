use super::{reserved_tier, Teammate};

const FLEET_CEILING: &str = "Opus 5.5 at medium";

const SIGNOFF_SHAPE: &str = "signoff: \"<yyyy-mm-dd> <who>: <why>\"";

const EFFORTS_ABOVE_CEILING: [&str; 3] = ["high", "xhigh", "max"];

const ORCHESTRATOR_BASE: &str = "fleet-orchestrator";

const ORCHESTRATION_ORCHESTRATOR: &str = "orchestration-orchestrator";

impl Teammate {
    pub fn is_orchestrator(&self) -> bool {
        self.base.as_deref() == Some(ORCHESTRATOR_BASE) || self.name == ORCHESTRATION_ORCHESTRATOR
    }

    pub fn effort_above_ceiling(&self) -> Option<&str> {
        self.effort
            .as_deref()
            .filter(|effort| EFFORTS_ABOVE_CEILING.contains(effort))
    }

    pub fn model_above_ceiling(&self) -> Option<&str> {
        self.model
            .as_deref()
            .filter(|model| !self.is_orchestrator() && reserved_tier(model).is_some())
    }

    pub fn above_ceiling(&self) -> bool {
        self.effort_above_ceiling().is_some() || self.model_above_ceiling().is_some()
    }
}

pub(crate) fn ceiling_problems(t: &Teammate) -> Vec<String> {
    let who = &t.name;
    match (&t.signoff, t.effort_above_ceiling()) {
        (None, Some(effort)) => vec![format!(
            "{who}: effort '{effort}' is above the fleet ceiling ({FLEET_CEILING}) and has no \
             operator sign-off; set effort: medium, or have the operator add {SIGNOFF_SHAPE}"
        )],
        (None, None) => Vec::new(),
        (Some(signoff), _) => signoff_shape_problem(signoff)
            .map(|why| format!("{who}: {why}; only the operator writes {SIGNOFF_SHAPE}"))
            .into_iter()
            .chain((!t.above_ceiling()).then(|| stale_signoff(t)))
            .collect(),
    }
}

fn stale_signoff(t: &Teammate) -> String {
    format!(
        "{}: signoff is set, but this teammate is not above the fleet ceiling \
         ({FLEET_CEILING}); remove it",
        t.name
    )
}

fn signoff_shape_problem(signoff: &str) -> Option<&'static str> {
    if signoff.trim().is_empty() {
        return Some("signoff is empty");
    }
    if signoff.contains('\n') {
        return Some("signoff must be one line");
    }
    let Some(signed) = after_date(signoff) else {
        return Some("signoff does not start with a yyyy-mm-dd date");
    };
    match signed.split_once(": ") {
        Some((who, why)) if !who.trim().is_empty() && !why.trim().is_empty() => None,
        Some((who, _)) if who.trim().is_empty() => Some("signoff does not say who signed"),
        _ => Some("signoff gives no reason after ': '"),
    }
}

fn after_date(signoff: &str) -> Option<&str> {
    let (date, rest) = (signoff.get(..10)?, signoff.get(10..)?);
    let shaped = date.bytes().enumerate().all(|(at, byte)| match at {
        4 | 7 => byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    let real = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok();
    let ends = rest.is_empty() || rest.starts_with(' ');
    (shaped && real && ends).then(|| rest.trim_start())
}

pub(crate) fn awaits_signoff(t: &Teammate) -> Option<String> {
    let model = t.model_above_ceiling().filter(|_| t.signoff.is_none())?;
    Some(format!(
        "{}: model '{model}' is above the fleet ceiling ({FLEET_CEILING}) and awaits the \
         operator's sign-off",
        t.name
    ))
}
