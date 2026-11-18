//! The shape and the limits of `result.json` of an attempt (ticket 12).
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol"
//! (Example: result.json; protocol rules 1, 2, 5 and 7). [`validate`] is the
//! schema: `horch result` calls it before it writes the file, and the ingest
//! of fleetd calls it before it reads one. No I/O: bytes in, a
//! [`ResultFile`] or a [`Refusal`] out. A domain module names only `std`, so
//! the JSON reader and writer of this schema are here.

/// The size limit of `result.json` (design, protocol rule 7). The key
/// `result_max_bytes` of `fleet.toml` has this starting value.
pub const RESULT_MAX_BYTES: u64 = 64 * 1024;

/// The closed list of statuses. The status is the claim of the worker:
/// only the verifier moves a task to `done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultStatus {
    Completed,
    Blocked,
    Failed,
}

impl ResultStatus {
    pub const ALL: [ResultStatus; 3] = [
        ResultStatus::Completed,
        ResultStatus::Blocked,
        ResultStatus::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ResultStatus::Completed => "completed",
            ResultStatus::Blocked => "blocked",
            ResultStatus::Failed => "failed",
        }
    }

    fn parse(word: &str) -> Option<ResultStatus> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }
}

/// 1 command that the worker ran to check its own work, and its exit code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfCheck {
    pub run: String,
    pub exit: i64,
}

/// A valid `result.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultFile {
    /// The task id of the path of the file.
    pub task: String,
    /// The attempt id of the path of the file, `A<n>`.
    pub attempt: String,
    pub status: ResultStatus,
    pub summary: String,
    pub commits: Vec<String>,
    pub files_changed: Vec<String>,
    pub acceptance_self_check: Vec<SelfCheck>,
    pub notes_for_next: Option<String>,
}

/// Why a `result.json` is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The file is over the size limit.
    TooLarge { bytes: u64, max: u64 },
    /// The bytes are not 1 JSON value in UTF-8.
    NotJson(String),
    /// The JSON value does not match the schema.
    Schema(String),
    /// The payload names another task than its path.
    OtherTask { payload: String, path: String },
    /// The payload names another attempt than its path.
    OtherAttempt { payload: String, path: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::TooLarge { bytes, max } => {
                write!(f, "result.json has {bytes} bytes; the limit is {max} bytes")
            }
            Refusal::NotJson(why) => write!(f, "result.json is not JSON: {why}"),
            Refusal::Schema(why) => write!(f, "result.json does not match its schema: {why}"),
            Refusal::OtherTask { payload, path } => write!(
                f,
                "result.json names task {payload:?}, but its path is of task {path:?}"
            ),
            Refusal::OtherAttempt { payload, path } => write!(
                f,
                "result.json names attempt {payload:?}, but its path is of attempt {path:?}"
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// Check `bytes` as `result.json` of attempt `attempt` of task `task`, the
/// ids of its path, with the size limit `max_bytes`.
pub fn validate(
    bytes: &[u8],
    task: &str,
    attempt: &str,
    max_bytes: u64,
) -> Result<ResultFile, Refusal> {
    let size = bytes.len() as u64;
    if size > max_bytes {
        return Err(Refusal::TooLarge {
            bytes: size,
            max: max_bytes,
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|e| Refusal::NotJson(e.to_string()))?;
    let value = Parser::new(text).document().map_err(Refusal::NotJson)?;
    let file = from_value(value).map_err(Refusal::Schema)?;
    if file.task != task {
        return Err(Refusal::OtherTask {
            payload: file.task,
            path: task.to_string(),
        });
    }
    if file.attempt != attempt {
        return Err(Refusal::OtherAttempt {
            payload: file.attempt,
            path: attempt.to_string(),
        });
    }
    Ok(file)
}

impl ResultFile {
    /// The JSON text of the file, 1 key on each line, in the order of the
    /// example of the design.
    pub fn to_json(&self) -> String {
        let strings = |items: &[String]| {
            let quoted: Vec<String> = items.iter().map(|s| json_string(s)).collect();
            format!("[{}]", quoted.join(", "))
        };
        let checks: Vec<String> = self
            .acceptance_self_check
            .iter()
            .map(|c| format!("{{\"run\": {}, \"exit\": {}}}", json_string(&c.run), c.exit))
            .collect();
        let notes = self
            .notes_for_next
            .as_deref()
            .map_or("null".to_string(), json_string);
        let lines = [
            format!("\"task\": {}", json_string(&self.task)),
            format!("\"attempt\": {}", json_string(&self.attempt)),
            format!("\"status\": {}", json_string(self.status.as_str())),
            format!("\"summary\": {}", json_string(&self.summary)),
            format!("\"commits\": {}", strings(&self.commits)),
            format!("\"files_changed\": {}", strings(&self.files_changed)),
            format!("\"acceptance_self_check\": [{}]", checks.join(", ")),
            format!("\"notes_for_next\": {notes}"),
        ];
        format!("{{\n  {}\n}}\n", lines.join(",\n  "))
    }
}

/// The keys of the schema. Each other key is refused.
const KEYS: [&str; 8] = [
    "task",
    "attempt",
    "status",
    "summary",
    "commits",
    "files_changed",
    "acceptance_self_check",
    "notes_for_next",
];

/// The [`ResultFile`] of a parsed JSON value.
fn from_value(value: Json) -> Result<ResultFile, String> {
    let Json::Obj(fields) = value else {
        return Err("the top level is not an object".into());
    };
    for (i, (key, _)) in fields.iter().enumerate() {
        if !KEYS.contains(&key.as_str()) {
            return Err(format!("unknown key {key:?}"));
        }
        if fields[..i].iter().any(|(k, _)| k == key) {
            return Err(format!("key {key:?} is given 2 times"));
        }
    }
    let get = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let required = |key: &str| match get(key) {
        Some(Json::Str(s)) => Ok(s.clone()),
        Some(_) => Err(format!("{key:?} is not a string")),
        None => Err(format!("{key:?} is missing")),
    };
    let status_word = required("status")?;
    let status = ResultStatus::parse(&status_word).ok_or_else(|| {
        format!("\"status\" is {status_word:?}; it must be completed, blocked or failed")
    })?;
    let notes_for_next = match get("notes_for_next") {
        None | Some(Json::Null) => None,
        Some(Json::Str(s)) => Some(s.clone()),
        Some(_) => return Err("\"notes_for_next\" is not a string or null".into()),
    };
    Ok(ResultFile {
        task: required("task")?,
        attempt: required("attempt")?,
        status,
        summary: required("summary")?,
        commits: string_list(get("commits"), "commits")?,
        files_changed: string_list(get("files_changed"), "files_changed")?,
        acceptance_self_check: self_checks(get("acceptance_self_check"))?,
        notes_for_next,
    })
}

/// An optional list of strings: missing is empty.
fn string_list(value: Option<&Json>, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Json::Arr(items) = value else {
        return Err(format!("{key:?} is not a list"));
    };
    items
        .iter()
        .map(|item| match item {
            Json::Str(s) => Ok(s.clone()),
            _ => Err(format!("an item of {key:?} is not a string")),
        })
        .collect()
}

/// The optional list `acceptance_self_check`: missing is empty.
fn self_checks(value: Option<&Json>) -> Result<Vec<SelfCheck>, String> {
    const KEY: &str = "acceptance_self_check";
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Json::Arr(items) = value else {
        return Err(format!("{KEY:?} is not a list"));
    };
    items.iter().map(self_check).collect()
}

/// 1 item of `acceptance_self_check`: `{"run": <string>, "exit": <integer>}`.
fn self_check(item: &Json) -> Result<SelfCheck, String> {
    let bad = || {
        "an item of \"acceptance_self_check\" is not {\"run\": <string>, \"exit\": <integer>}"
            .to_string()
    };
    let Json::Obj(fields) = item else {
        return Err(bad());
    };
    let mut run = None;
    let mut exit = None;
    for (key, value) in fields {
        match (key.as_str(), value) {
            ("run", Json::Str(s)) if run.is_none() => run = Some(s.clone()),
            ("exit", Json::Num(n)) if exit.is_none() => {
                exit = Some(n.parse::<i64>().map_err(|_| bad())?)
            }
            _ => return Err(bad()),
        }
    }
    match (run, exit) {
        (Some(run), Some(exit)) => Ok(SelfCheck { run, exit }),
        _ => Err(bad()),
    }
}

/// `s` as a JSON string.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON value. A number keeps its text: the schema reads only integers.
#[derive(Debug)]
enum Json {
    Null,
    Bool,
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// The nesting limit of the reader. `result.json` nests 3 levels, and a
/// limit keeps a hostile file from a deep recursion.
const MAX_DEPTH: usize = 16;

/// A reader of 1 JSON document (RFC 8259).
struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Parser<'a> {
        Parser { text, pos: 0 }
    }

    /// 1 value and only white space around it.
    fn document(mut self) -> Result<Json, String> {
        let value = self.value(0)?;
        self.space();
        if self.pos != self.text.len() {
            return Err(self.error("text after the value"));
        }
        Ok(value)
    }

    fn error(&self, what: &str) -> String {
        format!("{what} at byte {}", self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected {:?}", byte as char)))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err(self.error("the value nests too deep"));
        }
        self.space();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.word(),
            None => Err(self.error("the value is missing")),
        }
    }

    fn word(&mut self) -> Result<Json, String> {
        let rest = &self.text[self.pos..];
        for (word, value) in [
            ("null", Json::Null),
            ("true", Json::Bool),
            ("false", Json::Bool),
        ] {
            if rest.starts_with(word) {
                self.pos += word.len();
                return Ok(value);
            }
        }
        Err(self.error("unexpected character"))
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.expect(b':')?;
            fields.push((key, self.value(depth + 1)?));
            self.space();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let from = p.pos;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos > from
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else if !digits(self) {
            return Err(self.error("a number has no digits"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return Err(self.error("a fraction has no digits"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return Err(self.error("an exponent has no digits"));
            }
        }
        Ok(Json::Num(self.text[start..self.pos].to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.pos..];
            let Some(c) = rest.chars().next() else {
                return Err(self.error("a string has no end"));
            };
            self.pos += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c if (c as u32) < 0x20 => return Err(self.error("a control character in a string")),
                c => out.push(c),
            }
        }
    }

    /// The character of the escape after a `\`.
    fn escape(&mut self) -> Result<char, String> {
        let Some(byte) = self.peek() else {
            return Err(self.error("a string has no end"));
        };
        self.pos += 1;
        Ok(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode(),
            _ => return Err(self.error("an unknown escape")),
        })
    }

    /// The character of `\uXXXX`, or of a surrogate pair of 2 of them.
    fn unicode(&mut self) -> Result<char, String> {
        let first = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&first) {
            if !self.text[self.pos..].starts_with("\\u") {
                return Err(self.error("a lone surrogate"));
            }
            self.pos += 2;
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(self.error("a lone surrogate"));
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else {
            first
        };
        char::from_u32(code).ok_or_else(|| self.error("a lone surrogate"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let hex = self
            .text
            .get(self.pos..self.pos + 4)
            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("a \\u escape needs 4 hex digits"))?;
        self.pos += 4;
        u32::from_str_radix(hex, 16).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{
  "task": "T-0007-02",
  "attempt": "A1",
  "status": "completed",
  "summary": "Added retry logging in SyncClient._retry with attempt and delay.",
  "commits": ["3f9c2ab"],
  "files_changed": ["src/sync/client.py", "tests/test_backoff.py"],
  "acceptance_self_check": [{"run": "pytest tests/test_backoff.py -q -k logs", "exit": 0}],
  "notes_for_next": "Delay is logged in ms."
}"#;

    fn check(text: &str) -> Result<ResultFile, Refusal> {
        validate(text.as_bytes(), "T-0007-02", "A1", RESULT_MAX_BYTES)
    }

    #[test]
    fn the_example_of_the_design_is_valid() {
        let file = check(EXAMPLE).unwrap();
        assert_eq!(file.status, ResultStatus::Completed);
        assert_eq!(file.commits, ["3f9c2ab"]);
        assert_eq!(file.files_changed.len(), 2);
        assert_eq!(
            file.acceptance_self_check,
            [SelfCheck {
                run: "pytest tests/test_backoff.py -q -k logs".into(),
                exit: 0
            }]
        );
        assert_eq!(
            file.notes_for_next.as_deref(),
            Some("Delay is logged in ms.")
        );
    }

    #[test]
    fn result_json_with_another_status_is_refused() {
        for word in ["done", "Completed", "asked", ""] {
            let text = EXAMPLE.replace("\"completed\"", &format!("{word:?}"));
            match check(&text) {
                Err(Refusal::Schema(why)) => assert!(why.contains("status"), "{why}"),
                other => panic!("{word:?}: {other:?}"),
            }
        }
        let text = EXAMPLE.replace("\"completed\"", "1");
        assert!(matches!(check(&text), Err(Refusal::Schema(_))));
        for status in ResultStatus::ALL {
            let text = EXAMPLE.replace("completed", status.as_str());
            assert_eq!(check(&text).unwrap().status, status);
        }
    }

    #[test]
    fn the_ids_of_the_payload_equal_those_of_the_path() {
        assert_eq!(
            validate(EXAMPLE.as_bytes(), "T-9", "A1", RESULT_MAX_BYTES),
            Err(Refusal::OtherTask {
                payload: "T-0007-02".into(),
                path: "T-9".into()
            })
        );
        assert_eq!(
            validate(EXAMPLE.as_bytes(), "T-0007-02", "A2", RESULT_MAX_BYTES),
            Err(Refusal::OtherAttempt {
                payload: "A1".into(),
                path: "A2".into()
            })
        );
    }

    #[test]
    fn oversized_result_is_refused() {
        let max = EXAMPLE.len() as u64;
        assert!(validate(EXAMPLE.as_bytes(), "T-0007-02", "A1", max).is_ok());
        assert_eq!(
            validate(EXAMPLE.as_bytes(), "T-0007-02", "A1", max - 1),
            Err(Refusal::TooLarge {
                bytes: max,
                max: max - 1
            })
        );
    }

    #[test]
    fn the_schema_refuses_a_missing_key_an_unknown_key_and_a_wrong_type() {
        let cases = [
            EXAMPLE.replace("\"summary\"", "\"summery\""),
            EXAMPLE.replace("  \"summary\": \"Added retry logging in SyncClient._retry with attempt and delay.\",\n", ""),
            EXAMPLE.replace("[\"3f9c2ab\"]", "\"3f9c2ab\""),
            EXAMPLE.replace("[\"3f9c2ab\"]", "[1]"),
            EXAMPLE.replace("\"exit\": 0", "\"exit\": 0.5"),
            EXAMPLE.replace("\"exit\": 0", "\"exit\": \"0\""),
            EXAMPLE.replace("\"Delay is logged in ms.\"", "3"),
            EXAMPLE.replace("\"attempt\": \"A1\",", "\"attempt\": \"A1\", \"attempt\": \"A1\","),
            "[]".to_string(),
        ];
        for text in &cases {
            assert!(matches!(check(text), Err(Refusal::Schema(_))), "{text}");
        }
    }

    #[test]
    fn bytes_that_are_not_1_json_value_are_refused() {
        let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
        let cases: [&[u8]; 7] = [
            b"",
            b"{",
            b"{} {}",
            b"{\"task\": \"a\\x\"}",
            b"{\"task\": \"\xff\"}",
            b"{\"task\": \"\\ud800\"}",
            deep.as_bytes(),
        ];
        for bytes in cases {
            assert!(
                matches!(
                    validate(bytes, "T-0007-02", "A1", RESULT_MAX_BYTES),
                    Err(Refusal::NotJson(_))
                ),
                "{}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn optional_keys_may_be_missing() {
        let text = r#"{"task": "T-0007-02", "attempt": "A1", "status": "failed", "summary": "s"}"#;
        let file = check(text).unwrap();
        assert!(file.commits.is_empty() && file.files_changed.is_empty());
        assert!(file.acceptance_self_check.is_empty());
        assert_eq!(file.notes_for_next, None);
    }

    #[test]
    fn to_json_reads_back_as_the_same_file() {
        let mut file = check(EXAMPLE).unwrap();
        file.summary =
            "a \"quoted\" line\nand \\ a tab\t, a bell \u{7}, an e \u{e9} and \u{1F600}".into();
        file.notes_for_next = None;
        let text = file.to_json();
        assert!(text.ends_with("}\n"));
        assert_eq!(check(&text).unwrap(), file);
    }
}
