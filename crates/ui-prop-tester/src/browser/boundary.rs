use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
}

impl Url {
    pub fn parse(text: &str) -> Option<Url> {
        let (scheme, rest) = text.split_once(':')?;
        if scheme.is_empty()
            || !scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
        let scheme = scheme.to_ascii_lowercase();
        let rest = rest.split('#').next().unwrap_or_default();
        let (rest, query) = match rest.split_once('?') {
            Some((before, query)) => (before, Some(query.to_string())),
            None => (rest, None),
        };
        let Some(after_slashes) = rest.strip_prefix("//") else {
            return Some(Url {
                scheme,
                host: String::new(),
                port: None,
                path: rest.to_string(),
                query,
            });
        };
        let (authority, path) = match after_slashes.find('/') {
            Some(index) => (&after_slashes[..index], &after_slashes[index..]),
            None => (after_slashes, ""),
        };
        let authority = authority.rsplit('@').next().unwrap_or_default();
        let (host, port) = split_host_port(authority);
        let port = match port {
            Some(port) => Some(port.parse().ok()?),
            None => None,
        };
        Some(Url {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
            path: if path.is_empty() { "/".into() } else { path.into() },
            query,
        })
    }

    pub fn effective_port(&self) -> Option<u16> {
        self.port.or(match self.scheme.as_str() {
            "http" | "ws" => Some(80),
            "https" | "wss" => Some(443),
            _ => None,
        })
    }

    pub fn path_and_query(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{}", self.path, query),
            None => self.path.clone(),
        }
    }

    pub fn origin(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{}", self.scheme, self.host, port),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }
}

fn split_host_port(authority: &str) -> (&str, Option<&str>) {
    if authority.starts_with('[') {
        return match authority.find(']') {
            Some(end) => {
                let host = &authority[..=end];
                let port = authority[end + 1..].strip_prefix(':');
                (host, port.filter(|port| !port.is_empty()))
            }
            None => (authority, None),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() => (host, Some(port)),
        Some((host, _)) => (host, None),
        None => (authority, None),
    }
}

pub fn origin_url(origin: &str) -> Result<String, String> {
    let lowered = origin.to_ascii_lowercase();
    if lowered.starts_with("http://") || lowered.starts_with("https://") {
        let parsed = Url::parse(origin).ok_or_else(|| format!("{origin} is not a valid URL"))?;
        if parsed.host.is_empty() {
            return Err(format!("{origin} has no host"));
        }
        return Ok(origin.to_string());
    }
    if lowered.starts_with("file://") {
        return Ok(origin.to_string());
    }
    let path = Path::new(origin);
    let absolute = std::fs::canonicalize(path)
        .map_err(|error| format!("{origin} is neither an http(s) URL nor a readable file: {error}"))?;
    if !absolute.is_file() {
        return Err(format!("{origin} is not a file"));
    }
    Ok(file_url(&absolute))
}

pub fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    let mut encoded = String::new();
    for byte in text.bytes() {
        let keep = byte.is_ascii_alphanumeric() || b"/-._~:!$&'()*+,;=@".contains(&byte);
        if keep {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Boundary {
    SameOrigin {
        scheme: String,
        host: String,
        port: Option<u16>,
    },
    SameFile {
        path: String,
    },
    Patterns(Vec<MatchPattern>),
}

impl Boundary {
    pub fn new(origin_url: &str, allow_urls: &[String]) -> Result<Boundary, String> {
        if !allow_urls.is_empty() {
            let patterns = allow_urls
                .iter()
                .map(|text| MatchPattern::parse(text))
                .collect::<Result<Vec<_>, _>>()?;
            let boundary = Boundary::Patterns(patterns);
            if !boundary.allows(origin_url) {
                return Err(format!(
                    "the origin {origin_url} is outside every --allow-url pattern"
                ));
            }
            return Ok(boundary);
        }
        let url = Url::parse(origin_url).ok_or_else(|| format!("{origin_url} is not a URL"))?;
        match url.scheme.as_str() {
            "file" => Ok(Boundary::SameFile { path: url.path }),
            _ => Ok(Boundary::SameOrigin {
                port: url.effective_port(),
                scheme: url.scheme,
                host: url.host,
            }),
        }
    }

    pub fn allows(&self, text: &str) -> bool {
        let Some(url) = Url::parse(text) else {
            return false;
        };
        match self {
            Boundary::SameOrigin { scheme, host, port } => {
                &url.scheme == scheme && &url.host == host && &url.effective_port() == port
            }
            Boundary::SameFile { path } => url.scheme == "file" && &url.path == path,
            Boundary::Patterns(patterns) => patterns.iter().any(|pattern| pattern.matches(&url)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPattern {
    schemes: Vec<String>,
    host: HostPattern,
    port: PortPattern,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostPattern {
    Any,
    Exact(String),
    Subdomains(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PortPattern {
    Any,
    Exact(u16),
}

impl MatchPattern {
    pub fn parse(text: &str) -> Result<MatchPattern, String> {
        let invalid = |reason: &str| format!("--allow-url '{text}' is not a match pattern: {reason}");
        if text == "<all_urls>" {
            return Ok(MatchPattern {
                schemes: ["http", "https", "ws", "wss", "ftp", "file"]
                    .map(String::from)
                    .to_vec(),
                host: HostPattern::Any,
                port: PortPattern::Any,
                path: "/*".into(),
            });
        }
        let (scheme, rest) = text
            .split_once("://")
            .ok_or_else(|| invalid("it needs <scheme>://<host><path>"))?;
        let schemes = match scheme.to_ascii_lowercase().as_str() {
            "*" => vec!["http".to_string(), "https".to_string()],
            known @ ("http" | "https" | "ws" | "wss" | "ftp" | "file" | "urn") => {
                vec![known.to_string()]
            }
            _ => return Err(invalid("the scheme must be *, http, https, ws, wss, ftp or file")),
        };
        let slash = rest
            .find('/')
            .ok_or_else(|| invalid("it needs a path, for example /*"))?;
        let (authority, path) = (&rest[..slash], &rest[slash..]);
        let (host_text, port_text) = split_host_port(authority);
        let host = if schemes == ["file"] && host_text.is_empty() {
            HostPattern::Any
        } else if host_text == "*" {
            HostPattern::Any
        } else if let Some(domain) = host_text.strip_prefix("*.") {
            if domain.contains('*') {
                return Err(invalid("'*' may only start the host"));
            }
            HostPattern::Subdomains(domain.to_ascii_lowercase())
        } else if host_text.is_empty() {
            return Err(invalid("the host is empty"));
        } else if host_text.contains('*') {
            return Err(invalid("'*' may only start the host"));
        } else {
            HostPattern::Exact(host_text.to_ascii_lowercase())
        };
        let port = match port_text {
            None | Some("*") => PortPattern::Any,
            Some(port) => PortPattern::Exact(port.parse().map_err(|_| invalid("bad port"))?),
        };
        Ok(MatchPattern {
            schemes,
            host,
            port,
            path: path.to_string(),
        })
    }

    pub fn matches(&self, url: &Url) -> bool {
        if !self.schemes.contains(&url.scheme) {
            return false;
        }
        let host_ok = match &self.host {
            HostPattern::Any => true,
            HostPattern::Exact(host) => &url.host == host,
            HostPattern::Subdomains(domain) => {
                &url.host == domain || url.host.ends_with(&format!(".{domain}"))
            }
        };
        let port_ok = match self.port {
            PortPattern::Any => true,
            PortPattern::Exact(port) => url.effective_port() == Some(port),
        };
        host_ok && port_ok && glob(&self.path, &url.path_and_query())
    }
}

fn glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_keeps_scheme_host_and_port() {
        let boundary = Boundary::new("http://127.0.0.1:8080/start", &[]).unwrap();
        assert!(boundary.allows("http://127.0.0.1:8080/other?x=1"));
        assert!(!boundary.allows("http://127.0.0.1:8081/"));
        assert!(!boundary.allows("https://127.0.0.1:8080/"));
        assert!(!boundary.allows("http://localhost:8080/"));
        let default_port = Boundary::new("http://example.com/", &[]).unwrap();
        assert!(default_port.allows("http://example.com:80/a"));
    }

    #[test]
    fn same_file_allows_query_changes_only() {
        let boundary = Boundary::new("file:///tmp/site/index.html", &[]).unwrap();
        assert!(boundary.allows("file:///tmp/site/index.html?page=2#top"));
        assert!(!boundary.allows("file:///tmp/site/other.html"));
    }

    #[test]
    fn match_patterns_follow_the_extension_syntax() {
        let boundary = Boundary::new(
            "http://app.test:3000/",
            &["*://*.app.test/*".into(), "http://other.test:9000/docs/*".into()],
        )
        .unwrap();
        assert!(boundary.allows("https://api.app.test/v1"));
        assert!(boundary.allows("http://other.test:9000/docs/a?b"));
        assert!(!boundary.allows("http://other.test:9001/docs/a"));
        assert!(!boundary.allows("http://other.test:9000/blog"));
        assert!(Boundary::new("http://x.test/", &["http://y.test/*".into()]).is_err());
        assert!(MatchPattern::parse("http://*foo/*").is_err());
        let all = Boundary::new("file:///a.html", &["<all_urls>".into()]).unwrap();
        assert!(all.allows("https://anything.test/x"));
    }

    #[test]
    fn file_urls_escape_unsafe_bytes() {
        assert_eq!(
            file_url(Path::new("/tmp/a b/c#d.html")),
            "file:///tmp/a%20b/c%23d.html"
        );
    }
}
