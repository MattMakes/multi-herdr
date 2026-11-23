use std::collections::BTreeMap;

use super::plan::Plan;
use super::plan_validator::{PlanFacts, Problem};

pub const RULE: &str = "dag";

pub fn check(plan: &Plan, _facts: &PlanFacts) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut after: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for task in &plan.tasks {
        if after.contains_key(task.key.as_str()) {
            continue;
        }
        let mut known = Vec::new();
        for dep in &task.after {
            if plan.task(dep).is_some() {
                known.push(dep.as_str());
            } else {
                out.push(Problem::task(
                    RULE,
                    &task.key,
                    format!("after: `{dep}` is not a task of the plan"),
                ));
            }
        }
        after.insert(task.key.as_str(), known);
    }
    for cycle in cycles(&after) {
        let path = cycle.join(" -> ");
        out.push(Problem::task(
            RULE,
            cycle[0],
            format!("the after: links make a cycle: {path}"),
        ));
    }
    out
}

fn cycles<'a>(after: &BTreeMap<&'a str, Vec<&'a str>>) -> Vec<Vec<&'a str>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        OnPath,
        Done,
    }
    let mut mark: BTreeMap<&str, Mark> = after.keys().map(|k| (*k, Mark::New)).collect();
    let mut found = Vec::new();
    for &start in after.keys() {
        if mark[start] != Mark::New {
            continue;
        }
        let mut path: Vec<(&str, usize)> = vec![(start, 0)];
        mark.insert(start, Mark::OnPath);
        while let Some((key, next)) = path.last_mut() {
            let Some(&dep) = after[*key].get(*next) else {
                mark.insert(*key, Mark::Done);
                path.pop();
                continue;
            };
            *next += 1;
            match mark[dep] {
                Mark::New => {
                    mark.insert(dep, Mark::OnPath);
                    path.push((dep, 0));
                }
                Mark::OnPath => {
                    let at = path.iter().position(|(k, _)| *k == dep).unwrap_or(0);
                    let mut cycle: Vec<&str> = path[at..].iter().map(|(k, _)| *k).collect();
                    let min = (0..cycle.len()).min_by_key(|&i| cycle[i]).unwrap_or(0);
                    cycle.rotate_left(min);
                    cycle.push(cycle[0]);
                    found.push(cycle);
                }
                Mark::Done => {}
            }
        }
    }
    found
}
