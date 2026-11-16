use std::fmt::Write as _;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use unicode_width::UnicodeWidthStr;

use crate::cli::TraceArgs;
use crate::driver::DriverKind;
use crate::trace::{read_trace_file, Trace, TraceState, Violation};
use crate::{Io, EXIT_ERROR, EXIT_OK};

const CELL_TEXT_MAX: usize = 160;

pub fn run(args: &TraceArgs, io: &mut Io<'_>) -> i32 {
    match execute(args, io) {
        Ok(()) => EXIT_OK,
        Err(message) => {
            let _ = writeln!(io.err, "ui-prop-tester: error: {message}");
            EXIT_ERROR
        }
    }
}

fn execute(args: &TraceArgs, io: &mut Io<'_>) -> Result<(), String> {
    check_flags(args)?;
    let trace = load_trace(&args.trace)?;
    let range = match &args.state {
        Some(text) => parse_state_selection(text, trace.states.len())?,
        None => 0..trace.states.len(),
    };
    let selected = &trace.states[range];
    let written = if let Some(directory) = &args.screenshots {
        write_screenshots(&trace, selected, directory, io)
    } else if args.json {
        print_json(selected, io)
    } else if args.screen {
        print_screens(&trace, selected, args.color, io)
    } else {
        print_report(&args.trace, &trace, selected, io)
    };
    written.map_err(|error| format!("cannot write output: {error}"))?
}

fn check_flags(args: &TraceArgs) -> Result<(), String> {
    if args.screenshots.is_some() && (args.json || args.screen) {
        return Err("--screenshots cannot be combined with --json or --screen".into());
    }
    if args.json && args.screen {
        return Err("--json and --screen cannot be combined; choose one".into());
    }
    if args.color && !args.screen {
        return Err("--color only applies to --screen".into());
    }
    Ok(())
}

pub fn load_trace(path: &Path) -> Result<Trace, String> {
    read_trace_file(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => format!("trace file {} does not exist", path.display()),
        _ => format!("cannot read trace {}: {error}", path.display()),
    })
}

pub fn parse_state_selection(text: &str, count: usize) -> Result<Range<usize>, String> {
    let trimmed = text.trim();
    let malformed = || {
        format!("--state '{text}' is not an index or a START..END range (for example 3, 2..5, 4.. or ..6)")
    };
    let number = |part: &str| -> Result<usize, String> {
        part.trim().parse::<usize>().map_err(|_| malformed())
    };
    match trimmed.split_once("..") {
        None => {
            let index = number(trimmed)?;
            if index >= count {
                return Err(past_end(index, count));
            }
            Ok(index..index + 1)
        }
        Some((start, end)) => {
            let start = if start.trim().is_empty() {
                0
            } else {
                number(start)?
            };
            let end = if end.trim().is_empty() {
                count
            } else {
                number(end)?
            };
            if start > end {
                return Err(format!(
                    "--state '{text}' starts after it ends; write START..END with START <= END"
                ));
            }
            if end > count {
                return Err(format!(
                    "--state '{text}' ends past the last state; the trace has {count} states (0..{count})"
                ));
            }
            Ok(start..end)
        }
    }
}

fn past_end(index: usize, count: usize) -> String {
    match count {
        0 => format!("state {index} does not exist; the trace has no states"),
        _ => format!(
            "state {index} does not exist; the trace has {count} states (0 to {})",
            count - 1
        ),
    }
}

fn print_json(states: &[TraceState], io: &mut Io<'_>) -> io::Result<Result<(), String>> {
    for state in states {
        let mut value = serde_json::to_value(state).map_err(io::Error::other)?;
        if let Value::Object(object) = &mut value {
            object.insert(
                "has_screenshot".into(),
                Value::Bool(state.screenshot.is_some()),
            );
        }
        writeln!(io.out, "{value}")?;
    }
    Ok(Ok(()))
}

fn print_report(
    path: &Path,
    trace: &Trace,
    states: &[TraceState],
    io: &mut Io<'_>,
) -> io::Result<Result<(), String>> {
    let header = &trace.header;
    let mut text = String::new();
    let _ = writeln!(text, "trace          {}", path.display());
    let _ = writeln!(text, "driver         {}", driver_name(header.driver));
    let _ = writeln!(text, "target         {}", describe_target(trace));
    let _ = writeln!(text, "seed           {}", header.seed);
    let specification = match &header.specification {
        Some(record) => record.path.clone(),
        None => "driver defaults".into(),
    };
    let _ = writeln!(text, "specification  {specification}");
    let _ = writeln!(text, "states         {}", trace.states.len());
    let _ = writeln!(text, "outcome        {}", describe_outcome(trace));
    if states.len() != trace.states.len() {
        if let (Some(first), Some(last)) = (states.first(), states.last()) {
            let _ = writeln!(text, "showing        {} to {}", first.index, last.index);
        } else {
            let _ = writeln!(text, "showing        no states");
        }
    }
    for state in states {
        text.push('\n');
        write_state_block(&mut text, state);
    }
    io.out.write_all(text.as_bytes())?;
    Ok(Ok(()))
}

fn driver_name(kind: DriverKind) -> &'static str {
    match kind {
        DriverKind::Browser => "browser",
        DriverKind::Terminal => "terminal",
    }
}

fn describe_target(trace: &Trace) -> String {
    let target = &trace.header.target;
    match trace.header.driver {
        DriverKind::Browser => {
            let origin = target
                .get("origin")
                .and_then(Value::as_str)
                .unwrap_or("(unknown origin)");
            match (
                target.get("width").and_then(Value::as_u64),
                target.get("height").and_then(Value::as_u64),
            ) {
                (Some(width), Some(height)) => format!("{origin}  ({width}x{height})"),
                _ => origin.to_string(),
            }
        }
        DriverKind::Terminal => {
            let mut words: Vec<String> = Vec::new();
            if let Some(program) = target.get("program").and_then(Value::as_str) {
                words.push(shell_word(program));
            }
            if let Some(args) = target.get("args").and_then(Value::as_array) {
                words.extend(args.iter().filter_map(Value::as_str).map(shell_word));
            }
            let command = if words.is_empty() {
                "(unknown program)".to_string()
            } else {
                words.join(" ")
            };
            match (
                target.get("columns").and_then(Value::as_u64),
                target.get("rows").and_then(Value::as_u64),
            ) {
                (Some(columns), Some(rows)) => format!("{command}  ({columns}x{rows})"),
                _ => command,
            }
        }
    }
}

fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn describe_outcome(trace: &Trace) -> String {
    let Some(summary) = &trace.summary else {
        return "unfinished (the trace has no summary record)".into();
    };
    let mut text = summary.outcome.clone();
    let violations = summary.violations.len();
    if violations > 0 {
        let names = trace.violated_properties().join(", ");
        let noun = if violations == 1 {
            "violation"
        } else {
            "violations"
        };
        let _ = write!(text, " ({violations} {noun}: {names})");
    }
    if let Some(message) = &summary.message {
        let _ = write!(text, " - {message}");
    }
    text
}

fn write_state_block(text: &mut String, state: &TraceState) {
    let mut title = format!("state {}  +{} ms", state.index, state.timestamp_ms);
    if state.screenshot.is_some() {
        title.push_str("  [screenshot]");
    }
    match state.violations.len() {
        0 => {}
        1 => title.push_str("  [1 violation]"),
        count => {
            let _ = write!(title, "  [{count} violations]");
        }
    }
    let _ = writeln!(text, "{title}");
    let action = match &state.action {
        Some(action) => describe_action(action),
        None => "(initial state)".into(),
    };
    let _ = writeln!(text, "  action  {action}");
    match &state.cells {
        Value::Object(cells) if !cells.is_empty() => {
            let width = cells.keys().map(|name| name.width()).max().unwrap_or(0);
            let _ = writeln!(text, "  cells");
            for (name, value) in cells {
                let padding = " ".repeat(width - name.width());
                let _ = writeln!(text, "    {name}{padding}  {}", clip(&value.to_string()));
            }
        }
        Value::Object(_) | Value::Null => {}
        other => {
            let _ = writeln!(text, "  cells   {}", clip(&other.to_string()));
        }
    }
    for violation in &state.violations {
        write_violation(text, violation);
    }
}

fn write_violation(text: &mut String, violation: &Violation) {
    let _ = writeln!(text, "  VIOLATION {}", violation.property);
    let _ = writeln!(text, "    formula  {}", violation.path);
    if let Some(message) = &violation.message {
        let _ = writeln!(text, "    message  {message}");
    }
    match &violation.cells {
        Value::Object(cells) if !cells.is_empty() => {
            for (name, value) in cells {
                let _ = writeln!(text, "    cell     {name} = {}", clip(&value.to_string()));
            }
        }
        Value::Object(_) | Value::Null => {}
        other => {
            let _ = writeln!(text, "    cells    {}", clip(&other.to_string()));
        }
    }
}

fn clip(text: &str) -> String {
    if text.chars().count() <= CELL_TEXT_MAX {
        return text.to_string();
    }
    let kept: String = text.chars().take(CELL_TEXT_MAX).collect();
    format!("{kept}... ({} characters)", text.chars().count())
}

pub fn describe_action(action: &Value) -> String {
    let Value::Object(fields) = action else {
        return action.to_string();
    };
    let kind = fields
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("(no kind)");
    let mut parts = vec![kind.to_string()];
    if let Some(target) = fields.get("target") {
        parts.push(describe_fingerprint(target));
    }
    if let Some(point) = fields.get("point") {
        if let (Some(x), Some(y)) = (point.get("x"), point.get("y")) {
            parts.push(format!("at ({x}, {y})"));
        }
    }
    let skipped = ["kind", "target", "point", "category"];
    for (key, value) in fields {
        if skipped.contains(&key.as_str()) {
            continue;
        }
        parts.push(format!("{key}={}", clip(&value.to_string())));
    }
    if let Some(category) = fields.get("category").and_then(Value::as_str) {
        parts.push(format!("[{category}]"));
    }
    parts.join(" ")
}

fn describe_fingerprint(target: &Value) -> String {
    let Value::Object(fields) = target else {
        return target.to_string();
    };
    let text_of = |key: &str| fields.get(key).and_then(Value::as_str);
    let mut parts: Vec<String> = Vec::new();
    if let Some(test_id) = text_of("test_id") {
        parts.push(format!("[test_id={test_id}]"));
    }
    if let Some(id) = text_of("id") {
        parts.push(format!("#{id}"));
    }
    if let Some(role) = text_of("role") {
        parts.push(format!("role={role}"));
    }
    if let Some(name) = text_of("name").or_else(|| text_of("text")) {
        parts.push(format!("\"{}\"", clip_short(name)));
    }
    if let Some(href) = text_of("href") {
        parts.push(format!("href={href}"));
    }
    if parts.is_empty() {
        for key in ["form_name", "placeholder", "input_type", "path"] {
            if let Some(value) = text_of(key) {
                parts.push(format!("{key}={value}"));
                break;
            }
        }
    }
    if parts.is_empty() {
        return Value::Object(fields.clone()).to_string();
    }
    parts.join(" ")
}

fn clip_short(text: &str) -> String {
    let limit = 40;
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let kept: String = text.chars().take(limit).collect();
        format!("{kept}...")
    }
}

fn write_screenshots(
    trace: &Trace,
    states: &[TraceState],
    directory: &Path,
    io: &mut Io<'_>,
) -> io::Result<Result<(), String>> {
    if trace.header.driver != DriverKind::Browser {
        return Ok(Err(
            "--screenshots needs a browser trace; this is a terminal trace, draw it with --screen"
                .into(),
        ));
    }
    if directory.exists() && !directory.is_dir() {
        return Ok(Err(format!(
            "{} exists and is not a directory",
            directory.display()
        )));
    }
    let mut plan: Vec<(PathBuf, &[u8], bool)> = Vec::new();
    for state in states {
        let Some(png) = &state.screenshot else {
            continue;
        };
        let path = directory.join(format!("state-{:06}.png", state.index));
        let existing = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Ok(Err(format!("cannot read {}: {error}", path.display())));
            }
        };
        match existing {
            Some(bytes) if bytes == *png => plan.push((path, png, true)),
            Some(_) => {
                return Ok(Err(format!(
                    "{} already exists with different content; nothing was written",
                    path.display()
                )));
            }
            None => plan.push((path, png, false)),
        }
    }
    if let Err(error) = fs::create_dir_all(directory) {
        return Ok(Err(format!(
            "cannot create {}: {error}",
            directory.display()
        )));
    }
    let mut written = 0usize;
    let mut kept = 0usize;
    for (path, png, identical) in &plan {
        if *identical {
            kept += 1;
            writeln!(io.out, "kept     {}", path.display())?;
            continue;
        }
        if let Err(error) = fs::write(path, png) {
            return Ok(Err(format!("cannot write {}: {error}", path.display())));
        }
        written += 1;
        writeln!(io.out, "wrote    {}", path.display())?;
    }
    let skipped = states.len() - plan.len();
    writeln!(
        io.out,
        "{written} written, {kept} already present, {skipped} states without a screenshot"
    )?;
    Ok(Ok(()))
}

fn print_screens(
    trace: &Trace,
    states: &[TraceState],
    color: bool,
    io: &mut Io<'_>,
) -> io::Result<Result<(), String>> {
    if trace.header.driver != DriverKind::Terminal {
        return Ok(Err(
            "--screen draws terminal screens; for a browser trace use --screenshots <DIRECTORY> or `ui-prop-tester browser inspect`"
                .into(),
        ));
    }
    let mut first = true;
    for state in states {
        let mut text = String::new();
        if !first {
            text.push('\n');
        }
        first = false;
        let action = match &state.action {
            Some(action) => describe_action(action),
            None => "(initial state)".into(),
        };
        let _ = writeln!(
            text,
            "state {}  +{} ms  {action}",
            state.index, state.timestamp_ms
        );
        render_screen(&mut text, &state.state, color);
        for violation in &state.violations {
            write_violation(&mut text, violation);
        }
        io.out.write_all(text.as_bytes())?;
    }
    Ok(Ok(()))
}

#[derive(Clone, Default)]
struct Slot<'a> {
    text: String,
    attributes: Option<&'a Map<String, Value>>,
    continuation: bool,
}

pub fn render_screen(text: &mut String, state: &Value, color: bool) {
    let columns = state.get("columns").and_then(Value::as_u64).unwrap_or(0) as usize;
    let rows = state.get("rows").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut details = format!("{columns}x{rows}");
    if let Some(cursor) = state.get("cursor") {
        let row = cursor.get("row").and_then(Value::as_u64).unwrap_or(0);
        let column = cursor.get("column").and_then(Value::as_u64).unwrap_or(0);
        let visible = cursor
            .get("visible")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let shape = cursor
            .get("shape")
            .and_then(Value::as_str)
            .unwrap_or("block");
        let visibility = if visible { "visible" } else { "hidden" };
        let _ = write!(
            details,
            "  cursor row {row} column {column} ({visibility} {shape})"
        );
    }
    if let Some(offset) = state.get("scroll_offset").and_then(Value::as_u64) {
        if offset > 0 {
            let _ = write!(details, "  scrolled back {offset}");
        }
    }
    if let Some(exit) = state.get("exit").filter(|exit| !exit.is_null()) {
        match (
            exit.get("code").and_then(Value::as_i64),
            exit.get("signal").and_then(Value::as_i64),
        ) {
            (Some(code), _) => {
                let _ = write!(details, "  exited with code {code}");
            }
            (None, Some(signal)) => {
                let _ = write!(details, "  killed by signal {signal}");
            }
            _ => {
                let _ = write!(details, "  exited");
            }
        }
    }
    let _ = writeln!(text, "{details}");
    let grid = build_grid(state, columns, rows);
    let _ = writeln!(text, "\u{250c}{}\u{2510}", "\u{2500}".repeat(columns));
    for row in &grid {
        text.push('\u{2502}');
        render_row(text, row, color);
        text.push('\u{2502}');
        text.push('\n');
    }
    let _ = writeln!(text, "\u{2514}{}\u{2518}", "\u{2500}".repeat(columns));
}

fn build_grid(state: &Value, columns: usize, rows: usize) -> Vec<Vec<Slot<'_>>> {
    let mut grid: Vec<Vec<Slot<'_>>> = vec![vec![Slot::default(); columns]; rows];
    if let Some(lines) = state.get("lines").and_then(Value::as_array) {
        for (row, line) in lines.iter().enumerate().take(rows) {
            let Some(line) = line.as_str() else { continue };
            let mut column = 0usize;
            for character in line.chars() {
                let width = UnicodeWidthStr::width(character.to_string().as_str());
                if width == 0 {
                    if column > 0 {
                        let target = previous_base(&grid[row], column);
                        grid[row][target].text.push(character);
                    }
                    continue;
                }
                if column + width > columns {
                    break;
                }
                grid[row][column].text = character.to_string();
                if width == 2 {
                    grid[row][column + 1].continuation = true;
                }
                column += width;
            }
        }
    }
    if let Some(cells) = state.get("cells").and_then(Value::as_array) {
        for cell in cells {
            let Value::Object(fields) = cell else {
                continue;
            };
            let row = fields
                .get("row")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX) as usize;
            let column = fields
                .get("column")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX) as usize;
            if row >= rows || column >= columns {
                continue;
            }
            let slot = &mut grid[row][column];
            slot.attributes = Some(fields);
            if fields
                .get("wide_continuation")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                slot.continuation = true;
                slot.text.clear();
                continue;
            }
            if let Some(cell_text) = fields.get("text").and_then(Value::as_str) {
                slot.text = cell_text.to_string();
                slot.continuation = false;
                if cell_text.width() == 2 && column + 1 < columns {
                    grid[row][column + 1].continuation = true;
                }
            }
        }
    }
    grid
}

fn previous_base(row: &[Slot<'_>], column: usize) -> usize {
    let mut index = column - 1;
    while index > 0 && row[index].continuation {
        index -= 1;
    }
    index
}

fn render_row(text: &mut String, row: &[Slot<'_>], color: bool) {
    let columns = row.len();
    let mut current_style = String::new();
    let mut column = 0usize;
    while column < columns {
        let slot = &row[column];
        if slot.continuation {
            text.push(' ');
            column += 1;
            continue;
        }
        let style = if color {
            slot.attributes.map(sgr_parameters).unwrap_or_default()
        } else {
            String::new()
        };
        if style != current_style {
            text.push_str("\x1b[0m");
            if !style.is_empty() {
                let _ = write!(text, "\x1b[{style}m");
            }
            current_style = style;
        }
        let width = slot.text.width();
        if slot.text.is_empty() || width == 0 {
            text.push(' ');
            column += 1;
        } else if width >= 2 {
            if column + 1 < columns && row[column + 1].continuation {
                text.push_str(&slot.text);
                column += 2;
            } else {
                text.push(' ');
                column += 1;
            }
        } else {
            text.push_str(&slot.text);
            column += 1;
        }
    }
    if !current_style.is_empty() {
        text.push_str("\x1b[0m");
    }
}

fn sgr_parameters(fields: &Map<String, Value>) -> String {
    let flag = |name: &str| fields.get(name).and_then(Value::as_bool).unwrap_or(false);
    let mut codes: Vec<String> = Vec::new();
    if flag("bold") {
        codes.push("1".into());
    }
    if flag("dim") {
        codes.push("2".into());
    }
    if flag("italic") {
        codes.push("3".into());
    }
    if let Some(style) = fields.get("underline").and_then(Value::as_str) {
        let code = match style {
            "double" => "4:2",
            "curly" => "4:3",
            "dotted" => "4:4",
            "dashed" => "4:5",
            _ => "4",
        };
        codes.push(code.into());
    }
    if flag("blink") {
        codes.push("5".into());
    }
    if flag("inverse") {
        codes.push("7".into());
    }
    if flag("invisible") {
        codes.push("8".into());
    }
    if flag("strikethrough") {
        codes.push("9".into());
    }
    if flag("overline") {
        codes.push("53".into());
    }
    if let Some(code) = fields
        .get("fg")
        .and_then(|value| color_code(value, 30, 90, 38))
    {
        codes.push(code);
    }
    if let Some(code) = fields
        .get("bg")
        .and_then(|value| color_code(value, 40, 100, 48))
    {
        codes.push(code);
    }
    if let Some(code) = fields
        .get("underline_color")
        .and_then(|value| extended_color(value, 58))
    {
        codes.push(code);
    }
    codes.join(";")
}

fn color_code(value: &Value, base: u64, bright: u64, extended: u64) -> Option<String> {
    if let Some(index) = value.get("indexed").and_then(Value::as_u64) {
        return Some(match index {
            0..=7 => (base + index).to_string(),
            8..=15 => (bright + index - 8).to_string(),
            _ => format!("{extended};5;{}", index.min(255)),
        });
    }
    extended_color(value, extended)
}

fn extended_color(value: &Value, extended: u64) -> Option<String> {
    if let Some(index) = value.get("indexed").and_then(Value::as_u64) {
        return Some(format!("{extended};5;{}", index.min(255)));
    }
    let rgb = value.get("rgb").and_then(Value::as_array)?;
    let channel = |position: usize| {
        rgb.get(position)
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(255)
    };
    Some(format!(
        "{extended};2;{};{};{}",
        channel(0),
        channel(1),
        channel(2)
    ))
}
