use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    pub fn to_json(self) -> Value {
        match self {
            Color::Indexed(index) => json!({ "indexed": index }),
            Color::Rgb(red, green, blue) => json!({ "rgb": [red, green, blue] }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl Underline {
    pub fn name(self) -> Option<&'static str> {
        match self {
            Underline::None => None,
            Underline::Single => Some("single"),
            Underline::Double => Some("double"),
            Underline::Curly => Some("curly"),
            Underline::Dotted => Some("dotted"),
            Underline::Dashed => Some("dashed"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attributes {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub underline_color: Option<Color>,
    pub underline: Underline,
    pub bold: bool,
    pub italic: bool,
    pub blink: bool,
    pub inverse: bool,
    pub strikethrough: bool,
    pub dim: bool,
    pub invisible: bool,
    pub overline: bool,
}

impl Attributes {
    pub fn erased(self) -> Attributes {
        Attributes {
            bg: self.bg,
            ..Attributes::default()
        }
    }

    fn write_json(&self, object: &mut Map<String, Value>) {
        let colors = [
            ("fg", self.fg),
            ("bg", self.bg),
            ("underline_color", self.underline_color),
        ];
        for (name, color) in colors {
            if let Some(color) = color {
                object.insert(name.into(), color.to_json());
            }
        }
        if let Some(style) = self.underline.name() {
            object.insert("underline".into(), Value::from(style));
        }
        let flags = [
            ("bold", self.bold),
            ("italic", self.italic),
            ("blink", self.blink),
            ("inverse", self.inverse),
            ("strikethrough", self.strikethrough),
            ("dim", self.dim),
            ("invisible", self.invisible),
            ("overline", self.overline),
        ];
        for (name, set) in flags {
            if set {
                object.insert(name.into(), Value::Bool(true));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub wide_continuation: bool,
    pub attributes: Attributes,
}

impl Default for Cell {
    fn default() -> Self {
        Cell::blank(Attributes::default())
    }
}

impl Cell {
    pub fn blank(attributes: Attributes) -> Cell {
        Cell {
            text: " ".into(),
            wide_continuation: false,
            attributes,
        }
    }

    pub fn continuation(attributes: Attributes) -> Cell {
        Cell {
            text: String::new(),
            wide_continuation: true,
            attributes,
        }
    }

    pub fn is_default_blank(&self) -> bool {
        !self.wide_continuation && self.text == " " && self.attributes == Attributes::default()
    }

    pub fn to_json(&self, row: usize, column: usize) -> Value {
        let mut object = Map::new();
        object.insert("row".into(), Value::from(row));
        object.insert("column".into(), Value::from(column));
        object.insert("text".into(), Value::from(self.text.clone()));
        if self.wide_continuation {
            object.insert("wide_continuation".into(), Value::Bool(true));
        }
        self.attributes.write_json(&mut object);
        Value::Object(object)
    }
}
