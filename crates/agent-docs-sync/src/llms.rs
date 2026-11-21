pub fn links(text: &str, index_url: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].trim();
        if target.starts_with("http://") || target.starts_with("https://") {
            found.push(target.to_string());
        } else if target.starts_with('/') {
            found.push(format!("{}{target}", origin(index_url)));
        }
        rest = &rest[end..];
    }
    found
}

fn origin(url: &str) -> &str {
    let after_scheme = url.find("://").map_or(0, |i| i + 3);
    match url[after_scheme..].find('/') {
        Some(i) => &url[..after_scheme + i],
        None => url,
    }
}

pub fn pick<'a>(links: &'a [String], entry: &str, md_suffix: Option<&str>) -> Option<&'a str> {
    let tail = format!("/{entry}");
    links
        .iter()
        .filter(|link| bare_path(link, md_suffix).ends_with(&tail))
        .min_by_key(|link| bare_path(link, md_suffix).matches('/').count())
        .map(String::as_str)
}

fn bare_path<'a>(link: &'a str, md_suffix: Option<&str>) -> &'a str {
    let link = link.split(['?', '#']).next().unwrap_or(link);
    let link = md_suffix
        .and_then(|suffix| link.strip_suffix(suffix))
        .unwrap_or(link);
    let link = link.strip_suffix(".md").unwrap_or(link);
    link.trim_end_matches('/')
}

pub fn page_url(link: &str, md_suffix: Option<&str>) -> String {
    let link = link.split('#').next().unwrap_or(link);
    match md_suffix {
        Some(suffix) if !link.ends_with(suffix) => format!("{link}{suffix}"),
        _ => link.to_string(),
    }
}

pub fn file_name(entry: &str) -> String {
    format!("{}.md", entry.replace('/', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_absolute_and_in_order() {
        let text =
            "- [A](https://x.dev/docs/a.md): a\n- [B](/docs/b) and [C](c.md)\n[D](https://x.dev/d";
        assert_eq!(
            links(text, "https://x.dev/docs/llms.txt"),
            ["https://x.dev/docs/a.md", "https://x.dev/docs/b"]
        );
    }

    #[test]
    fn pick_matches_whole_segments_and_prefers_the_shortest_path() {
        let all: Vec<String> = [
            "https://x.dev/docs/en/sdk/hooks.md",
            "https://x.dev/docs/en/hooks.md",
            "https://x.dev/docs/en/webhooks.md",
            "https://x.dev/docs/cli/reference#flags",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            pick(&all, "hooks", Some(".md")),
            Some("https://x.dev/docs/en/hooks.md")
        );
        assert_eq!(
            pick(&all, "cli/reference", None),
            Some("https://x.dev/docs/cli/reference#flags")
        );
        assert_eq!(pick(&all, "reference/x", None), None);
    }

    #[test]
    fn page_url_adds_the_suffix_once() {
        assert_eq!(
            page_url("https://x.dev/a", Some(".md")),
            "https://x.dev/a.md"
        );
        assert_eq!(
            page_url("https://x.dev/a.md", Some(".md")),
            "https://x.dev/a.md"
        );
        assert_eq!(page_url("https://x.dev/a", None), "https://x.dev/a");
        assert_eq!(
            page_url("https://x.dev/a#b", Some(".md")),
            "https://x.dev/a.md"
        );
    }

    #[test]
    fn file_name_flattens_the_entry() {
        assert_eq!(file_name("cli/reference"), "cli-reference.md");
        assert_eq!(file_name("hooks"), "hooks.md");
    }
}
