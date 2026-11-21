use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{json, Value};

#[derive(Debug, Default)]
pub struct Coverage {
    branches: HashSet<(String, u64, u64)>,
    functions: BTreeMap<String, BTreeSet<(u64, String)>>,
}

impl Coverage {
    pub fn record(&mut self, taken: &Value) -> u64 {
        let mut added = 0;
        let scripts = taken.get("result").and_then(Value::as_array);
        for script in scripts.into_iter().flatten() {
            let url = script
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if url.is_empty() {
                continue;
            }
            let functions = script.get("functions").and_then(Value::as_array);
            for function in functions.into_iter().flatten() {
                let ranges = function.get("ranges").and_then(Value::as_array);
                let mut first = true;
                for range in ranges.into_iter().flatten() {
                    let count = range.get("count").and_then(Value::as_u64).unwrap_or(0);
                    let start = range
                        .get("startOffset")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let end = range.get("endOffset").and_then(Value::as_u64).unwrap_or(0);
                    if count > 0 {
                        if self.branches.insert((url.to_string(), start, end)) {
                            added += 1;
                        }
                        if first {
                            let name = function
                                .get("functionName")
                                .and_then(Value::as_str)
                                .filter(|name| !name.is_empty())
                                .unwrap_or("<anonymous>");
                            self.functions
                                .entry(url.to_string())
                                .or_default()
                                .insert((start, name.to_string()));
                        }
                    }
                    first = false;
                }
            }
        }
        added
    }

    pub fn total(&self) -> u64 {
        self.branches.len() as u64
    }

    pub fn symbols(&self) -> Value {
        let scripts: Vec<Value> = self
            .functions
            .iter()
            .map(|(url, functions)| {
                let mut branches: Vec<(u64, u64)> = self
                    .branches
                    .iter()
                    .filter(|(branch_url, _, _)| branch_url == url)
                    .map(|(_, start, end)| (*start, *end))
                    .collect();
                branches.sort_unstable();
                json!({
                    "url": url,
                    "functions": functions
                        .iter()
                        .map(|(offset, name)| json!({"name": name, "offset": offset}))
                        .collect::<Vec<_>>(),
                    "branches": branches
                        .iter()
                        .map(|(start, end)| json!([start, end]))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({ "scripts": scripts })
    }
}
