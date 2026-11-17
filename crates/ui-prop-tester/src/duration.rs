use std::time::Duration;

pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let trimmed = text.trim();
    let split = trimmed
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    let value: f64 = number
        .parse()
        .map_err(|_| format!("'{text}' is not a duration; use forms like 500ms, 30s, 5m or 1h"))?;
    let seconds = match unit.trim() {
        "ms" => value / 1000.0,
        "" | "s" => value,
        "m" => value * 60.0,
        "h" => value * 3600.0,
        other => {
            return Err(format!(
                "'{other}' is not a duration unit; use ms, s, m or h"
            ))
        }
    };
    Ok(Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_unit() {
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
    }

    #[test]
    fn rejects_unknown_units_and_empty_numbers() {
        assert!(parse_duration("5d").is_err());
        assert!(parse_duration("fast").is_err());
        assert!(parse_duration("").is_err());
    }
}
