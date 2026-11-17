use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

pub fn cookie_params(set_cookie: &str, origin_url: &str) -> Result<Value, String> {
    let mut parts = set_cookie.split(';');
    let pair = parts.next().unwrap_or_default();
    let (name, value) = pair
        .split_once('=')
        .ok_or_else(|| format!("--cookie '{set_cookie}' needs NAME=VALUE first"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("--cookie '{set_cookie}' has an empty name"));
    }
    let mut params = Map::new();
    params.insert("name".into(), json!(name));
    params.insert("value".into(), json!(value.trim()));
    let mut max_age: Option<f64> = None;
    let mut expires: Option<f64> = None;
    for attribute in parts {
        let (key, argument) = match attribute.split_once('=') {
            Some((key, argument)) => (key.trim(), Some(argument.trim())),
            None => (attribute.trim(), None),
        };
        match (key.to_ascii_lowercase().as_str(), argument) {
            ("", _) => {}
            ("domain", Some(domain)) => {
                params.insert("domain".into(), json!(domain));
            }
            ("path", Some(path)) => {
                params.insert("path".into(), json!(path));
            }
            ("secure", _) => {
                params.insert("secure".into(), json!(true));
            }
            ("httponly", _) => {
                params.insert("httpOnly".into(), json!(true));
            }
            ("samesite", Some(mode)) => {
                let mode = match mode.to_ascii_lowercase().as_str() {
                    "strict" => "Strict",
                    "lax" => "Lax",
                    "none" => "None",
                    _ => return Err(format!("--cookie '{set_cookie}' has SameSite={mode}")),
                };
                params.insert("sameSite".into(), json!(mode));
            }
            ("max-age", Some(seconds)) => {
                max_age = Some(
                    seconds
                        .parse()
                        .map_err(|_| format!("--cookie '{set_cookie}' has Max-Age={seconds}"))?,
                );
            }
            ("expires", Some(date)) => {
                expires = Some(
                    parse_http_date(date)
                        .ok_or_else(|| format!("--cookie '{set_cookie}' has Expires={date}"))?,
                );
            }
            _ => {}
        }
    }
    if let Some(seconds) = max_age {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |elapsed| elapsed.as_secs_f64());
        params.insert("expires".into(), json!(now + seconds));
    } else if let Some(expires) = expires {
        params.insert("expires".into(), json!(expires));
    }
    if !params.contains_key("domain") {
        let lowered = origin_url.to_ascii_lowercase();
        if !(lowered.starts_with("http://") || lowered.starts_with("https://")) {
            return Err(format!(
                "--cookie '{set_cookie}' needs a Domain attribute because the origin is not an http(s) URL"
            ));
        }
        params.insert("url".into(), json!(origin_url));
    } else if !params.contains_key("path") {
        params.insert("path".into(), json!("/"));
    }
    Ok(Value::Object(params))
}

fn parse_http_date(text: &str) -> Option<f64> {
    let tokens: Vec<&str> = text
        .split([' ', ',', '-'])
        .filter(|token| !token.is_empty())
        .collect();
    let mut day = None;
    let mut month = None;
    let mut year = None;
    let mut time = None;
    for token in tokens {
        if token.contains(':') {
            let pieces: Vec<i64> = token.split(':').filter_map(|p| p.parse().ok()).collect();
            if pieces.len() == 3 {
                time = Some(pieces[0] * 3600 + pieces[1] * 60 + pieces[2]);
            }
        } else if let Ok(number) = token.parse::<i64>() {
            if day.is_none() && number <= 31 && token.len() <= 2 {
                day = Some(number);
            } else {
                year = Some(if number < 100 { number + 2000 } else { number });
            }
        } else if let Some(index) = [
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        ]
        .iter()
        .position(|name| token.to_ascii_lowercase().starts_with(name))
        {
            month = Some(index as i64 + 1);
        }
    }
    let days = days_from_civil(year?, month?, day?);
    Some((days * 86_400 + time.unwrap_or(0)) as f64)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cookie_text_becomes_set_cookie_params() {
        let params = cookie_params(
            "session=abc; Path=/app; HttpOnly; SameSite=Lax; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
            "http://127.0.0.1:9000/",
        )
        .unwrap();
        assert_eq!(params["name"], "session");
        assert_eq!(params["url"], "http://127.0.0.1:9000/");
        assert_eq!(params["path"], "/app");
        assert_eq!(params["httpOnly"], true);
        assert_eq!(params["sameSite"], "Lax");
        assert_eq!(params["expires"], 1_445_412_480.0);
        let scoped = cookie_params("a=1; Domain=localhost", "file:///x.html").unwrap();
        assert_eq!(scoped["domain"], "localhost");
        assert_eq!(scoped["path"], "/");
        assert!(cookie_params("a=1", "file:///x.html").is_err());
        assert!(cookie_params("novalue", "http://x.test/").is_err());
    }
}
