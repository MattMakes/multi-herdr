use super::cell::{Attributes, Color, Underline};

pub fn apply(attributes: &mut Attributes, params: &[Vec<u16>]) {
    if params.is_empty() {
        *attributes = Attributes::default();
        return;
    }
    let mut index = 0;
    while index < params.len() {
        let param = &params[index];
        index += 1;
        let code = param.first().copied().unwrap_or(0);
        match code {
            0 => *attributes = Attributes::default(),
            1 => attributes.bold = true,
            2 => attributes.dim = true,
            3 => attributes.italic = true,
            4 => attributes.underline = underline_style(param.get(1).copied()),
            5 | 6 => attributes.blink = true,
            7 => attributes.inverse = true,
            8 => attributes.invisible = true,
            9 => attributes.strikethrough = true,
            21 => attributes.underline = Underline::Double,
            22 => {
                attributes.bold = false;
                attributes.dim = false;
            }
            23 => attributes.italic = false,
            24 => attributes.underline = Underline::None,
            25 => attributes.blink = false,
            27 => attributes.inverse = false,
            28 => attributes.invisible = false,
            29 => attributes.strikethrough = false,
            30..=37 => attributes.fg = Some(Color::Indexed((code - 30) as u8)),
            38 => attributes.fg = extended_color(param, params, &mut index),
            39 => attributes.fg = None,
            40..=47 => attributes.bg = Some(Color::Indexed((code - 40) as u8)),
            48 => attributes.bg = extended_color(param, params, &mut index),
            49 => attributes.bg = None,
            53 => attributes.overline = true,
            55 => attributes.overline = false,
            58 => attributes.underline_color = extended_color(param, params, &mut index),
            59 => attributes.underline_color = None,
            90..=97 => attributes.fg = Some(Color::Indexed((code - 90 + 8) as u8)),
            100..=107 => attributes.bg = Some(Color::Indexed((code - 100 + 8) as u8)),
            _ => {}
        }
    }
}

fn underline_style(style: Option<u16>) -> Underline {
    match style {
        None | Some(1) => Underline::Single,
        Some(2) => Underline::Double,
        Some(3) => Underline::Curly,
        Some(4) => Underline::Dotted,
        Some(5) => Underline::Dashed,
        Some(_) => Underline::None,
    }
}

fn byte(value: u16) -> u8 {
    value.min(255) as u8
}

fn extended_color(param: &[u16], params: &[Vec<u16>], index: &mut usize) -> Option<Color> {
    if param.len() > 1 {
        return match param[1] {
            5 => param.get(2).map(|&value| Color::Indexed(byte(value))),
            2 => {
                let components = if param.len() >= 6 {
                    &param[3..6]
                } else {
                    param.get(2..5)?
                };
                Some(Color::Rgb(
                    byte(components[0]),
                    byte(components[1]),
                    byte(components[2]),
                ))
            }
            _ => None,
        };
    }
    let next = |offset: usize| -> Option<u16> { params.get(*index + offset)?.first().copied() };
    match next(0)? {
        5 => {
            let value = next(1)?;
            *index += 2;
            Some(Color::Indexed(byte(value)))
        }
        2 => {
            let (red, green, blue) = (next(1)?, next(2)?, next(3)?);
            *index += 4;
            Some(Color::Rgb(byte(red), byte(green), byte(blue)))
        }
        _ => {
            *index += 1;
            None
        }
    }
}
