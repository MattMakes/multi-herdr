use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::sandbox::{Network, Rule, SANDBOX_EXEC};
use crate::NotSupported;

pub(crate) fn program(_path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    if !Path::new(SANDBOX_EXEC).is_file() {
        return Err(
            NotSupported::new("sandbox", "macos").because("/usr/bin/sandbox-exec is missing")
        );
    }
    Ok(PathBuf::from(SANDBOX_EXEC))
}

pub(crate) fn args(
    rules: &[Rule<'_>],
    _temp: bool,
    network: Network,
    _own_pids: bool,
) -> Vec<OsString> {
    let mut profile = String::from(
        "(version 1)(allow default)(deny file-write*)(allow file-write* (literal \"/dev/null\"))",
    );
    let mut args = Vec::new();
    for (n, rule) in rules.iter().enumerate() {
        let (path, text) = match rule {
            Rule::Read(path) => (
                path,
                format!("(allow file-read* (subpath (param \"P{n}\")))(deny file-write* (subpath (param \"P{n}\")))"),
            ),
            Rule::Write(path) => (
                path,
                format!("(allow file-read* file-write* (subpath (param \"P{n}\")))"),
            ),
            Rule::Hide(path) => (
                path,
                format!(
                    "(deny file-read* file-write* (subpath (param \"P{n}\")))\
                     (deny network-outbound (remote unix-socket (subpath (param \"P{n}\"))))"
                ),
            ),
        };
        profile.push_str(&text);
        let mut value = OsString::from(format!("P{n}="));
        value.push(path);
        args.extend([OsString::from("-D"), value]);
    }
    for (n, folder) in hidden_parents(rules).into_iter().enumerate() {
        profile.push_str(&format!(
            "(allow file-read-metadata (literal (param \"M{n}\")))"
        ));
        let mut value = OsString::from(format!("M{n}="));
        value.push(folder);
        args.extend([OsString::from("-D"), value]);
    }
    match network {
        Network::Open => {}
        Network::Denied => profile.push_str("(deny network*)"),
        Network::Local => profile.push_str(
            "(deny network-outbound (remote ip))\
             (deny network-bind (local ip))\
             (deny network-inbound (local ip))\
             (deny network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))",
        ),
    }
    args.extend([OsString::from("-p"), OsString::from(profile)]);
    args
}

fn hidden_parents<'a>(rules: &[Rule<'a>]) -> Vec<&'a Path> {
    let mut folders = Vec::new();
    for rule in rules {
        let (Rule::Read(path) | Rule::Write(path)) = *rule else {
            continue;
        };
        for folder in path.ancestors().skip(1) {
            let reaching = rules.iter().rev().find(|r| folder.starts_with(r.path()));
            if matches!(reaching, Some(Rule::Hide(_))) && !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    }
    folders
}
