use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ident,
    Number,
    Str,
    Template,
    Regex,
    Punct,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
    newline_before: bool,
}

const MULTI_PUNCT: &[&str] = &[
    "===", "!==", "**=", "...", "&&=", "||=", "??=", "=>", "==", "!=", "&&", "||", "??", "?.",
    "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**",
];

const REGEX_AFTER_KEYWORDS: &[&str] = &[
    "return",
    "typeof",
    "case",
    "do",
    "else",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "yield",
    "await",
    "instanceof",
];

const NOT_EXPRESSION_END: &[&str] = &[
    "return",
    "typeof",
    "case",
    "do",
    "else",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "yield",
    "await",
    "instanceof",
    "export",
    "import",
    "default",
    "extends",
    "as",
    "satisfies",
    "let",
    "const",
    "var",
    "if",
    "while",
    "for",
    "switch",
    "catch",
    "function",
    "class",
    "async",
    "static",
    "get",
    "set",
];

const PARAMETER_MODIFIERS: &[&str] = &["public", "private", "protected", "readonly", "override"];

const MEMBER_MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "readonly",
    "declare",
    "abstract",
    "override",
];

const CONTROL_KEYWORDS: &[&str] = &["if", "while", "for", "switch", "with", "return", "typeof"];

pub fn strip_types(source: &str) -> Result<String, String> {
    let tokens = lex(source)?;
    let matches = match_brackets(source, &tokens)?;
    let mut stripper = Stripper {
        source,
        tokens,
        matches,
        removed: Vec::new(),
        removed_ends: HashMap::new(),
        class_bodies: HashSet::new(),
        specifier_braces: HashSet::new(),
    };
    stripper.removed = vec![false; stripper.tokens.len()];
    stripper.run();
    Ok(stripper.render())
}

fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut position = 0;
    let mut newline_before = false;
    while position < bytes.len() {
        let rest = &source[position..];
        let c = rest.chars().next().unwrap_or(' ');
        if c == '\n' {
            newline_before = true;
            position += 1;
            continue;
        }
        if c.is_whitespace() || c == '\u{feff}' {
            position += c.len_utf8();
            continue;
        }
        if rest.starts_with("//") {
            position += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if let Some(body) = rest.strip_prefix("/*") {
            let end = body
                .find("*/")
                .ok_or_else(|| "a block comment is not closed".to_string())?;
            if body[..end].contains('\n') {
                newline_before = true;
            }
            position += end + 4;
            continue;
        }
        let start = position;
        let kind = if is_ident_start(c) {
            position += ident_length(rest);
            Kind::Ident
        } else if c.is_ascii_digit()
            || (c == '.' && rest[1..].starts_with(|n: char| n.is_ascii_digit()))
        {
            position += number_length(rest);
            Kind::Number
        } else if c == '"' || c == '\'' {
            position += string_length(rest, c)?;
            Kind::Str
        } else if c == '`' {
            position += template_length(rest)?;
            Kind::Template
        } else if c == '/' && regex_allowed(source, tokens.last()) {
            position += regex_length(rest)?;
            Kind::Regex
        } else {
            let length = MULTI_PUNCT
                .iter()
                .find(|punct| rest.starts_with(**punct))
                .map(|punct| punct.len())
                .unwrap_or(c.len_utf8());
            position += length;
            Kind::Punct
        };
        tokens.push(Token {
            kind,
            start,
            end: position,
            newline_before,
        });
        newline_before = false;
    }
    Ok(tokens)
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c == '#' || c.is_alphabetic()
}

fn ident_length(rest: &str) -> usize {
    rest.char_indices()
        .find(|&(index, c)| {
            !(c == '_' || c == '$' || c.is_alphanumeric() || (index == 0 && c == '#'))
        })
        .map(|(index, _)| index)
        .unwrap_or(rest.len())
}

fn number_length(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let b = bytes[index];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' {
            if (b == b'e' || b == b'E')
                && !rest.starts_with("0x")
                && matches!(bytes.get(index + 1), Some(b'+') | Some(b'-'))
            {
                index += 2;
                continue;
            }
            index += 1;
        } else {
            break;
        }
    }
    index
}

fn string_length(rest: &str, quote: char) -> Result<usize, String> {
    let mut chars = rest.char_indices().skip(1);
    while let Some((index, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c == quote {
            return Ok(index + 1);
        } else if c == '\n' {
            break;
        }
    }
    Err("a string literal is not closed".into())
}

fn template_length(rest: &str) -> Result<usize, String> {
    let bytes = rest.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'`' => return Ok(index + 1),
            b'$' if bytes.get(index + 1) == Some(&b'{') => {
                index += 2 + braced_length(&rest[index + 2..])?;
            }
            _ => index += 1,
        }
    }
    Err("a template literal is not closed".into())
}

fn braced_length(rest: &str) -> Result<usize, String> {
    let tokens = lex_until_close(rest)?;
    Ok(tokens)
}

fn lex_until_close(rest: &str) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut position = 0;
    while position < rest.len() {
        let tail = &rest[position..];
        let c = tail.chars().next().unwrap_or(' ');
        match c {
            '{' => {
                depth += 1;
                position += 1;
            }
            '}' => {
                if depth == 0 {
                    return Ok(position + 1);
                }
                depth -= 1;
                position += 1;
            }
            '"' | '\'' => position += string_length(tail, c)?,
            '`' => position += template_length(tail)?,
            _ if tail.starts_with("//") => position += tail.find('\n').unwrap_or(tail.len()),
            _ if tail.starts_with("/*") => {
                position += tail[2..]
                    .find("*/")
                    .map(|end| end + 4)
                    .ok_or_else(|| "a block comment is not closed".to_string())?
            }
            _ => position += c.len_utf8(),
        }
    }
    Err("a template expression is not closed".into())
}

fn regex_allowed(source: &str, previous: Option<&Token>) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    let text = &source[previous.start..previous.end];
    match previous.kind {
        Kind::Punct => !matches!(text, ")" | "]" | "}"),
        Kind::Ident => REGEX_AFTER_KEYWORDS.contains(&text),
        _ => false,
    }
}

fn regex_length(rest: &str) -> Result<usize, String> {
    let bytes = rest.as_bytes();
    let mut index = 1;
    let mut in_class = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'[' => {
                in_class = true;
                index += 1;
            }
            b']' => {
                in_class = false;
                index += 1;
            }
            b'/' if !in_class => {
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                return Ok(index);
            }
            b'\n' => break,
            _ => index += 1,
        }
    }
    Err("a regular expression literal is not closed".into())
}

fn match_brackets(source: &str, tokens: &[Token]) -> Result<Vec<usize>, String> {
    let mut matches = vec![usize::MAX; tokens.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        let text = &source[token.start..token.end];
        match text {
            "(" | "[" | "{" => stack.push(index),
            ")" | "]" | "}" => {
                let open = stack
                    .pop()
                    .ok_or_else(|| format!("an unmatched {text} at byte {}", token.start))?;
                matches[open] = index;
                matches[index] = open;
            }
            _ => {}
        }
    }
    if let Some(open) = stack.pop() {
        return Err(format!(
            "an unclosed bracket at byte {}",
            tokens[open].start
        ));
    }
    Ok(matches)
}

struct Stripper<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    matches: Vec<usize>,
    removed: Vec<bool>,
    removed_ends: HashMap<usize, usize>,
    class_bodies: HashSet<usize>,
    specifier_braces: HashSet<usize>,
}

impl Stripper<'_> {
    fn text(&self, index: usize) -> &str {
        match self.tokens.get(index) {
            Some(token) => &self.source[token.start..token.end],
            None => "",
        }
    }

    fn is(&self, index: usize, text: &str) -> bool {
        self.tokens.get(index).is_some_and(|token| {
            matches!(token.kind, Kind::Punct | Kind::Ident)
                && &self.source[token.start..token.end] == text
        })
    }

    fn is_ident(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == Kind::Ident)
    }

    fn kind(&self, index: usize) -> Option<Kind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    fn remove(&mut self, start: usize, end: usize) {
        if start >= end || end > self.tokens.len() {
            return;
        }
        for flag in &mut self.removed[start..end] {
            *flag = true;
        }
        let entry = self.removed_ends.entry(start).or_insert(end);
        *entry = (*entry).max(end);
    }

    fn previous_kept(&self, index: usize) -> Option<usize> {
        (0..index).rev().find(|&candidate| !self.removed[candidate])
    }

    fn ends_expression(&self, index: Option<usize>) -> bool {
        let Some(index) = index else {
            return false;
        };
        match self.tokens[index].kind {
            Kind::Number | Kind::Str | Kind::Template | Kind::Regex => true,
            Kind::Ident => !NOT_EXPRESSION_END.contains(&self.text(index)),
            Kind::Punct => matches!(self.text(index), ")" | "]" | "}"),
        }
    }

    fn at_statement_start(&self, index: usize) -> bool {
        if self.tokens[index].newline_before {
            return true;
        }
        match self.previous_kept(index) {
            None => true,
            Some(previous) => matches!(self.text(previous), ";" | "}" | "{" | "export" | "declare"),
        }
    }

    fn run(&mut self) {
        let mut stack: Vec<usize> = Vec::new();
        let mut index = 0;
        while index < self.tokens.len() {
            if let Some(&end) = self.removed_ends.get(&index) {
                index = end;
                continue;
            }
            if self.tokens[index].kind == Kind::Ident {
                self.ident_rules(index, &stack);
                if let Some(&end) = self.removed_ends.get(&index) {
                    index = end;
                    continue;
                }
            } else if self.tokens[index].kind == Kind::Punct {
                self.punct_rules(index);
                if let Some(&end) = self.removed_ends.get(&index) {
                    index = end;
                    continue;
                }
                match self.text(index) {
                    "(" | "[" | "{" => stack.push(index),
                    ")" | "]" | "}" => {
                        stack.pop();
                    }
                    _ => {}
                }
            }
            index += 1;
        }
    }

    fn ident_rules(&mut self, index: usize, stack: &[usize]) {
        let in_class_body = stack
            .last()
            .is_some_and(|open| self.class_bodies.contains(open));
        if in_class_body && self.member_start(index, *stack.last().unwrap_or(&0)) {
            self.class_member(index);
            if self.removed[index] {
                return;
            }
        }
        match self.text(index) {
            "import" => self.import_statement(index),
            "export" => self.export_statement(index),
            "interface" => self.interface_declaration(index),
            "type" => self.type_alias(index),
            "class" => self.class_header(index),
            "let" | "const" | "var" => self.variable_declaration(index),
            "as" | "satisfies" => self.cast(index),
            _ => {}
        }
    }

    fn punct_rules(&mut self, index: usize) {
        match self.text(index) {
            "<" => self.angle(index),
            "(" => self.parenthesis(index),
            "!" => self.non_null(index),
            _ => {}
        }
    }

    fn statement_end(&self, mut index: usize) -> usize {
        while index < self.tokens.len() {
            if self.is(index, ";") {
                return index + 1;
            }
            if self.tokens[index].newline_before {
                return index;
            }
            if matches!(self.text(index), "(" | "[" | "{") {
                index = self.matches[index] + 1;
                continue;
            }
            index += 1;
        }
        index
    }

    fn import_statement(&mut self, index: usize) {
        let default_import_named_type = self.is(index + 2, "from")
            && self.kind(index + 3) == Some(Kind::Str)
            || self.is(index + 2, ",")
            || self.is(index + 2, "(");
        if self.is(index + 1, "type") && !default_import_named_type {
            let end = self.statement_end(index + 1);
            self.remove(index, end);
            return;
        }
        let mut cursor = index + 1;
        if self.is_ident(cursor) && !self.is(cursor, "type") {
            cursor += 1;
            if self.is(cursor, ",") {
                cursor += 1;
            }
        }
        if self.is(cursor, "{") {
            self.specifiers(cursor);
        }
    }

    fn specifiers(&mut self, open: usize) {
        self.specifier_braces.insert(open);
        let close = self.matches[open];
        let mut cursor = open + 1;
        while cursor < close {
            if self.is(cursor, "type") && self.is_ident(cursor + 1) && !self.is(cursor + 1, "as") {
                let mut end = cursor + 2;
                if self.is(end, "as") {
                    end += 2;
                }
                if self.is(end, ",") {
                    end += 1;
                }
                self.remove(cursor, end);
                cursor = end;
                continue;
            }
            cursor += 1;
        }
    }

    fn export_statement(&mut self, index: usize) {
        if self.is(index + 1, "type") && self.is(index + 2, "{") {
            let close = self.matches[index + 2];
            let mut end = close + 1;
            if self.is(end, "from") {
                end += 2;
            }
            if self.is(end, ";") {
                end += 1;
            }
            self.remove(index, end);
            return;
        }
        if self.is(index + 1, "{") {
            self.specifiers(index + 1);
        }
    }

    fn declaration_start(&self, index: usize) -> usize {
        let mut start = index;
        while let Some(previous) = start.checked_sub(1) {
            if matches!(self.text(previous), "export" | "declare") && !self.removed[previous] {
                start = previous;
            } else {
                break;
            }
        }
        start
    }

    fn interface_declaration(&mut self, index: usize) {
        if !self.is_ident(index + 1) || !self.at_statement_start(index) {
            return;
        }
        let mut cursor = index + 2;
        while cursor < self.tokens.len() && !self.is(cursor, "{") {
            if self.is(cursor, ";") || self.is(cursor, "(") {
                return;
            }
            cursor += 1;
        }
        if cursor >= self.tokens.len() {
            return;
        }
        let end = self.matches[cursor] + 1;
        let start = self.declaration_start(index);
        self.remove(start, end);
    }

    fn type_alias(&mut self, index: usize) {
        if !self.is_ident(index + 1) || !self.at_statement_start(index) {
            return;
        }
        let mut cursor = index + 2;
        if self.is(cursor, "<") {
            match self.type_parameters(cursor) {
                Some(after) => cursor = after,
                None => return,
            }
        }
        if !self.is(cursor, "=") {
            return;
        }
        let Some(mut end) = self.parse_type(cursor + 1) else {
            return;
        };
        if self.is(end, ";") {
            end += 1;
        }
        let start = self.declaration_start(index);
        self.remove(start, end);
    }

    fn class_header(&mut self, index: usize) {
        let mut cursor = index + 1;
        if self.is_ident(cursor) && !self.is(cursor, "extends") && !self.is(cursor, "implements") {
            cursor += 1;
        }
        if self.is(cursor, "<") {
            if let Some(after) = self.type_parameters(cursor) {
                self.remove(cursor, after);
                cursor = after;
            }
        }
        while cursor < self.tokens.len() && !self.is(cursor, "{") {
            if self.is(cursor, "implements") {
                let mut end = cursor + 1;
                while end < self.tokens.len() && !self.is(end, "{") {
                    end += 1;
                }
                self.remove(cursor, end);
                cursor = end;
                break;
            }
            if self.is(cursor, "<") && self.previous_kept(cursor).is_some_and(|p| self.is_ident(p))
            {
                if let Some(after) = self.type_arguments(cursor) {
                    self.remove(cursor, after);
                    cursor = after;
                    continue;
                }
            }
            if matches!(self.text(cursor), "(" | "[") {
                cursor = self.matches[cursor] + 1;
                continue;
            }
            cursor += 1;
        }
        if self.is(cursor, "{") {
            self.class_bodies.insert(cursor);
        }
    }

    fn member_start(&self, index: usize, body: usize) -> bool {
        match self.previous_kept(index) {
            Some(previous) => {
                previous == body
                    || self.is(previous, ";")
                    || self.is(previous, "}")
                    || self.tokens[index].newline_before
            }
            None => false,
        }
    }

    fn class_member(&mut self, index: usize) {
        let mut cursor = index;
        while MEMBER_MODIFIERS.contains(&self.text(cursor))
            && (self.is_ident(cursor + 1) || self.is(cursor + 1, "["))
        {
            self.remove(cursor, cursor + 1);
            cursor += 1;
        }
        while matches!(self.text(cursor), "static" | "async" | "get" | "set")
            && (self.is_ident(cursor + 1) || self.is(cursor + 1, "["))
        {
            cursor += 1;
        }
        if self.is(cursor, "[") {
            cursor = self.matches[cursor] + 1;
        } else if self.is_ident(cursor) || self.kind(cursor) == Some(Kind::Str) {
            cursor += 1;
        } else {
            return;
        }
        let mut annotation = cursor;
        if self.is(annotation, "?") || self.is(annotation, "!") {
            annotation += 1;
        }
        if self.is(annotation, ":") {
            if let Some(end) = self.parse_type(annotation + 1) {
                self.remove(cursor, end);
            }
        } else if annotation > cursor && (self.is(annotation, ";") || self.is(annotation, "=")) {
            self.remove(cursor, annotation);
        }
    }

    fn variable_declaration(&mut self, index: usize) {
        let mut cursor = index + 1;
        if self.is(cursor, "{") || self.is(cursor, "[") {
            cursor = self.matches[cursor] + 1;
        } else if self.is_ident(cursor) {
            cursor += 1;
        } else {
            return;
        }
        let mut annotation = cursor;
        if self.is(annotation, "!") {
            annotation += 1;
        }
        if !self.is(annotation, ":") {
            return;
        }
        if let Some(end) = self.parse_type(annotation + 1) {
            let fits = end >= self.tokens.len()
                || matches!(self.text(end), "=" | ";" | "," | ")" | "in" | "of")
                || self.tokens[end].newline_before;
            if fits {
                self.remove(cursor, end);
            }
        }
    }

    fn cast(&mut self, index: usize) {
        if !self.ends_expression(self.previous_kept(index)) {
            return;
        }
        let previous = self.previous_kept(index).unwrap_or(0);
        if self.inside_specifiers(index) || self.is(previous, "*") {
            return;
        }
        if let Some(end) = self.parse_type(index + 1) {
            self.remove(index, end);
        }
    }

    fn inside_specifiers(&self, index: usize) -> bool {
        self.specifier_braces
            .iter()
            .any(|&open| open < index && index < self.matches[open])
    }

    fn non_null(&mut self, index: usize) {
        if self.tokens[index].newline_before {
            return;
        }
        let previous = self.previous_kept(index);
        let ends = previous.is_some_and(|p| {
            (self.is_ident(p) && !NOT_EXPRESSION_END.contains(&self.text(p)))
                || matches!(self.text(p), ")" | "]")
        });
        if !ends {
            return;
        }
        let next_starts_operand = self.tokens.get(index + 1).is_some_and(|next| {
            matches!(
                next.kind,
                Kind::Ident | Kind::Number | Kind::Str | Kind::Template | Kind::Regex
            ) && !next.newline_before
        });
        if !next_starts_operand {
            self.remove(index, index + 1);
        }
    }

    fn angle(&mut self, index: usize) {
        let previous = self.previous_kept(index);
        let after_name = previous.is_some_and(|p| {
            self.is_ident(p) && !NOT_EXPRESSION_END.contains(&self.text(p))
                || self.is(p, "function")
        });
        if after_name {
            let end = self
                .type_arguments(index)
                .filter(|&end| self.is(end, "("))
                .or_else(|| self.type_parameters(index).filter(|&end| self.is(end, "(")));
            if let Some(end) = end {
                self.remove(index, end);
            }
            return;
        }
        if self.ends_expression(previous) {
            return;
        }
        if let Some(end) = self.type_parameters(index) {
            if self.is(end, "(") && self.arrow_after(self.matches[end] + 1) {
                self.remove(index, end);
            }
        }
    }

    fn arrow_after(&self, index: usize) -> bool {
        if self.is(index, "=>") {
            return true;
        }
        if self.is(index, ":") {
            if let Some(end) = self.parse_type(index + 1) {
                return self.is(end, "=>");
            }
        }
        false
    }

    fn parenthesis(&mut self, index: usize) {
        let close = self.matches[index];
        let previous = self.previous_kept(index);
        let previous_text = previous.map(|p| self.text(p)).unwrap_or("");
        let after_function = previous_text == "function"
            || previous_text == "*"
            || previous.is_some_and(|p| {
                self.is_ident(p)
                    && self
                        .previous_kept(p)
                        .is_some_and(|q| matches!(self.text(q), "function" | "*"))
            });
        let catch_clause = previous_text == "catch";
        let arrow = self.arrow_after(close + 1);
        let method = previous.is_some_and(|p| {
            (self.is_ident(p) && !CONTROL_KEYWORDS.contains(&self.text(p)) || self.is(p, "]"))
                && (self.is(close + 1, "{")
                    || (self.is(close + 1, ":")
                        && self
                            .parse_type(close + 2)
                            .is_some_and(|end| self.is(end, "{"))))
        });
        if !(after_function || catch_clause || arrow || method) {
            return;
        }
        self.parameters(index, close);
        if !catch_clause && self.is(close + 1, ":") {
            if let Some(end) = self.parse_type(close + 2) {
                if self.is(end, "{") || self.is(end, "=>") {
                    self.remove(close + 1, end);
                }
            }
        }
    }

    fn parameters(&mut self, open: usize, close: usize) {
        let mut cursor = open + 1;
        while cursor < close {
            while PARAMETER_MODIFIERS.contains(&self.text(cursor))
                && (self.is_ident(cursor + 1)
                    || self.is(cursor + 1, "{")
                    || self.is(cursor + 1, "["))
            {
                self.remove(cursor, cursor + 1);
                cursor += 1;
            }
            if self.is(cursor, "...") {
                cursor += 1;
            }
            if self.is(cursor, "{") || self.is(cursor, "[") {
                cursor = self.matches[cursor] + 1;
            } else if self.is_ident(cursor) {
                cursor += 1;
            }
            let mut annotation = cursor;
            if self.is(annotation, "?") {
                annotation += 1;
            }
            if self.is(annotation, ":") {
                if let Some(end) = self.parse_type(annotation + 1) {
                    if end <= close {
                        self.remove(cursor, end);
                        cursor = end;
                    }
                }
            } else if annotation > cursor
                && (self.is(annotation, ",")
                    || self.is(annotation, ")")
                    || self.is(annotation, "="))
            {
                self.remove(cursor, annotation);
                cursor = annotation;
            }
            while cursor < close && !self.is(cursor, ",") {
                if matches!(self.text(cursor), "(" | "[" | "{") {
                    cursor = self.matches[cursor] + 1;
                } else {
                    cursor += 1;
                }
            }
            cursor += 1;
        }
    }

    fn parse_type(&self, index: usize) -> Option<usize> {
        let mut cursor = index;
        if self.is(cursor, "|") || self.is(cursor, "&") {
            cursor += 1;
        }
        cursor = self.parse_postfix(cursor)?;
        while self.is(cursor, "|") || self.is(cursor, "&") {
            cursor = self.parse_postfix(cursor + 1)?;
        }
        if self.is(cursor, "is") && self.tokens.get(cursor).is_some() {
            return self.parse_type(cursor + 1);
        }
        Some(cursor)
    }

    fn parse_postfix(&self, index: usize) -> Option<usize> {
        let mut cursor = self.parse_primary(index)?;
        while self.is(cursor, "[") && !self.tokens[cursor].newline_before {
            cursor = self.matches[cursor] + 1;
        }
        Some(cursor)
    }

    fn parse_primary(&self, index: usize) -> Option<usize> {
        let token = self.tokens.get(index)?;
        match token.kind {
            Kind::Str | Kind::Number | Kind::Template => Some(index + 1),
            Kind::Regex => None,
            Kind::Punct => match self.text(index) {
                "(" => {
                    let close = self.matches[index];
                    if self.is(close + 1, "=>") {
                        self.parse_type(close + 2)
                    } else {
                        Some(close + 1)
                    }
                }
                "{" | "[" => Some(self.matches[index] + 1),
                "-" if self.kind(index + 1) == Some(Kind::Number) => Some(index + 2),
                "<" => {
                    let after = self.type_parameters(index)?;
                    if !self.is(after, "(") {
                        return None;
                    }
                    let close = self.matches[after];
                    if self.is(close + 1, "=>") {
                        self.parse_type(close + 2)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            Kind::Ident => match self.text(index) {
                "typeof" => self.qualified_name(index + 1),
                "keyof" | "readonly" | "infer" | "asserts" => self.parse_postfix(index + 1),
                "unique" if self.is(index + 1, "symbol") => Some(index + 2),
                "new" if self.is(index + 1, "(") => {
                    let close = self.matches[index + 1];
                    if self.is(close + 1, "=>") {
                        self.parse_type(close + 2)
                    } else {
                        None
                    }
                }
                _ => self.qualified_name(index),
            },
        }
    }

    fn qualified_name(&self, index: usize) -> Option<usize> {
        if !self.is_ident(index) {
            return None;
        }
        let mut cursor = index + 1;
        while self.is(cursor, ".") && self.is_ident(cursor + 1) {
            cursor += 2;
        }
        if self.is(cursor, "<") {
            cursor = self.type_arguments(cursor)?;
        }
        Some(cursor)
    }

    fn type_arguments(&self, index: usize) -> Option<usize> {
        let mut cursor = index + 1;
        if self.is(cursor, ">") {
            return None;
        }
        loop {
            cursor = self.parse_type(cursor)?;
            if self.is(cursor, ",") {
                cursor += 1;
                continue;
            }
            if self.is(cursor, ">") {
                return Some(cursor + 1);
            }
            return None;
        }
    }

    fn type_parameters(&self, index: usize) -> Option<usize> {
        let mut cursor = index + 1;
        loop {
            while matches!(self.text(cursor), "const" | "in" | "out") && self.is_ident(cursor + 1) {
                cursor += 1;
            }
            if !self.is_ident(cursor) {
                return None;
            }
            cursor += 1;
            if self.is(cursor, "extends") {
                cursor = self.parse_type(cursor + 1)?;
            }
            if self.is(cursor, "=") {
                cursor = self.parse_type(cursor + 1)?;
            }
            if self.is(cursor, ",") {
                cursor += 1;
                if self.is(cursor, ">") {
                    return Some(cursor + 1);
                }
                continue;
            }
            if self.is(cursor, ">") {
                return Some(cursor + 1);
            }
            return None;
        }
    }

    fn render(&self) -> String {
        let mut blank = vec![false; self.source.len()];
        let mut index = 0;
        while index < self.tokens.len() {
            if !self.removed[index] {
                index += 1;
                continue;
            }
            let mut end = index;
            while end < self.tokens.len() && self.removed[end] {
                end += 1;
            }
            let from = self.tokens[index].start;
            let to = self.tokens[end - 1].end;
            for flag in &mut blank[from..to] {
                *flag = true;
            }
            index = end;
        }
        let mut out = Vec::with_capacity(self.source.len());
        for (position, byte) in self.source.bytes().enumerate() {
            if blank[position] && byte != b'\n' {
                out.push(b' ');
            } else {
                out.push(byte);
            }
        }
        String::from_utf8(out).unwrap_or_else(|_| self.source.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::strip_types;

    fn squash(text: &str) -> String {
        text.split_whitespace().collect::<String>()
    }

    fn check(input: &str, expected: &str) {
        let out = strip_types(input).unwrap();
        assert_eq!(squash(&out), squash(expected), "from {input:?}");
        assert_eq!(
            out.matches('\n').count(),
            input.matches('\n').count(),
            "line count changed for {input:?}"
        );
    }

    #[test]
    fn parameter_and_return_annotations_go() {
        check(
            "function add(a: number, b?: string): number { return a; }",
            "function add(a , b ) { return a; }",
        );
        check(
            "const f = (x: number, y: Array<string> = []): boolean => x > 0;",
            "const f = (x , y = []) => x > 0;",
        );
        check(
            "const g = async ({ a, b }: Props, ...rest: number[]) => a;",
            "const g = async ({ a, b } , ...rest ) => a;",
        );
        check(
            "const h = (cb: (value: number) => void): void => cb(1);",
            "const h = (cb ) => cb(1);",
        );
    }

    #[test]
    fn variable_annotations_go_but_object_literals_stay() {
        check(
            "let count: number = 0; const names: string[] = [];",
            "let count = 0; const names = [];",
        );
        check(
            "const point: { x: number; y: number } = { x: 1, y: 2 };",
            "const point = { x: 1, y: 2 };",
        );
        check("const a = cond ? b : c;", "const a = cond ? b : c;");
        check("let value!: string;", "let value ;");
    }

    #[test]
    fn interfaces_types_and_type_imports_go() {
        check(
            "export interface State { count: number; items: Array<string> }\nexport const x = 1;",
            "export const x = 1;",
        );
        check(
            "type Pair<T> = [T, T];\ntype Mode = \"a\" | \"b\";\nconst y = 2;",
            "const y = 2;",
        );
        check(
            "import type { State } from \"./state\";\nimport { always, type Cell } from \"ui-prop-tester\";",
            "import { always, } from \"ui-prop-tester\";",
        );
        check("export type { State } from \"./state\";\nlet z;", "let z;");
    }

    #[test]
    fn casts_generics_and_non_null_go() {
        check(
            "const n = (value as unknown as number) + 1;",
            "const n = (value ) + 1;",
        );
        check(
            "const m = new Map<string, number>();",
            "const m = new Map();",
        );
        check(
            "const v = pick<string>(items)!.length;",
            "const v = pick(items).length;",
        );
        check("const w = items[0]!;", "const w = items[0];");
        check("const keep = a !== b && !c;", "const keep = a !== b && !c;");
        check(
            "function first<T extends object>(list: T[]): T | undefined { return list[0]; }",
            "function first(list ) { return list[0]; }",
        );
        check("const id = <T,>(x: T) => x;", "const id = (x ) => x;");
        check(
            "const frozen = { a: 1 } as const;",
            "const frozen = { a: 1 } ;",
        );
        check(
            "import { a as b } from \"x\";\nexport { b as c };",
            "import { a as b } from \"x\";\nexport { b as c };",
        );
    }

    #[test]
    fn strings_templates_regexes_and_comparisons_survive() {
        check(
            "const s = \"a: number\"; const t = `x: ${1 + 2}: y`; const r = /a:b/g;",
            "const s = \"a: number\"; const t = `x: ${1 + 2}: y`; const r = /a:b/g;",
        );
        check(
            "if (a < b && c > d) { go(); }",
            "if (a < b && c > d) { go(); }",
        );
        check(
            "for (let i = 0; i < n; i++) {}",
            "for (let i = 0; i < n; i++) {}",
        );
    }

    #[test]
    fn class_members_lose_annotations_and_modifiers() {
        check(
            "class Box<T> implements Thing {\n  private value: T;\n  readonly size?: number = 1;\n  constructor(public name: string) { this.value = null; }\n  get(): T { return this.value; }\n}",
            "class Box {\n value;\n size = 1;\n constructor( name ) { this.value = null; }\n get() { return this.value; }\n}",
        );
    }

    #[test]
    fn object_methods_and_catch_clauses_lose_annotations() {
        check(
            "const o = { run(x: number): void { return; }, other: 1 };",
            "const o = { run(x ) { return; }, other: 1 };",
        );
        check(
            "try { go(); } catch (error: unknown) { log(error); }",
            "try { go(); } catch (error ) { log(error); }",
        );
    }

    #[test]
    fn unbalanced_input_is_an_error() {
        assert!(strip_types("function f( {").is_err());
        assert!(strip_types("const s = \"open").is_err());
    }
}
