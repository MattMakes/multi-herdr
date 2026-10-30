//! `horch self-update` paths that `self_update_cli.rs` leaves out: `--check`,
//! up to date, no release, a failing install and a missing asset. A local
//! server stands in for GitHub; the environment is cleared.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn archive_name(tag: &str) -> String {
    format!("multi-herdr-{tag}-{}.tar.gz", env!("HORCH_TARGET"))
}

fn tar_gz(script: &str) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);
    for name in ["horch", "multi-herdr-dataset", "herdr-install"] {
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, script.as_bytes())
            .unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

/// What the server offers for `/releases/latest`.
enum Offer {
    /// 404: no release yet.
    None,
    /// `tag` with the archive and, when `sums`, its SHA256SUMS.
    Release { tag: &'static str, sums: bool },
}

/// Serve the offer and `archive`; returns the API base URL.
fn serve(offer: Offer, archive: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tag, sums) = match offer {
        Offer::None => ("", false),
        Offer::Release { tag, sums } => (tag, sums),
    };
    let name = archive_name(tag);
    let mut assets = vec![format!(
        r#"{{"name":"{name}","browser_download_url":"{base}/dl/{name}"}}"#
    )];
    if sums {
        assets.push(format!(
            r#"{{"name":"SHA256SUMS","browser_download_url":"{base}/dl/SHA256SUMS"}}"#
        ));
    }
    let release = format!(
        r#"{{"tag_name":"{tag}","html_url":"{base}/release","assets":[{}]}}"#,
        assets.join(",")
    );
    let digest: String = Sha256::digest(&archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sums_body = format!("{digest}  {name}\n");
    let none = tag.is_empty();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let path = request.split_whitespace().nth(1).unwrap_or("");
            let (status, body): (&str, Vec<u8>) = match path {
                "/releases/latest" if !none => ("200 OK", release.clone().into_bytes()),
                "/dl/SHA256SUMS" => ("200 OK", sums_body.clone().into_bytes()),
                p if p == format!("/dl/{name}") => ("200 OK", archive.clone()),
                _ => ("404 Not Found", b"{}".to_vec()),
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    base
}

fn world() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for dir in ["home", "data", "state", "bin"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    (tmp, root)
}

fn self_update(root: &Path, api: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_horch"))
        .arg("self-update")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_RELEASE_API", api)
        .env("HORCH_SELF_UPDATE_DIR", root.join("bin"))
        .output()
        .unwrap()
}

fn texts(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn marker_script(root: &Path, exit: u8) -> String {
    format!(
        "#!/bin/sh\necho \"$@\" > '{}'\nexit {exit}\n",
        root.join("marker").display()
    )
}

#[test]
fn check_reports_a_newer_release_and_installs_nothing() {
    let (_tmp, root) = world();
    let api = serve(
        Offer::Release {
            tag: "v99.0.0",
            sums: true,
        },
        tar_gz(&marker_script(&root, 0)),
    );
    let out = self_update(&root, &api, &["--check"]);
    let (stdout, stderr) = texts(&out);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(
        stdout,
        format!("multi-herdr v99.0.0 is out (you have v{CURRENT}): {api}/release\n")
    );
    assert!(!root.join("marker").exists());
}

#[test]
fn an_older_release_is_up_to_date() {
    let (_tmp, root) = world();
    let api = serve(
        Offer::Release {
            tag: "v0.0.1",
            sums: true,
        },
        tar_gz(&marker_script(&root, 0)),
    );
    let out = self_update(&root, &api, &[]);
    let (stdout, stderr) = texts(&out);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(stdout, format!("horch is up to date (v{CURRENT})\n"));
    assert!(!root.join("marker").exists());
}

#[test]
fn no_release_is_not_an_error() {
    let (_tmp, root) = world();
    let api = serve(Offer::None, Vec::new());
    let out = self_update(&root, &api, &[]);
    let (stdout, stderr) = texts(&out);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(
        stdout,
        format!("horch: no multi-herdr release is published yet (you have v{CURRENT})\n")
    );
}

#[test]
fn a_failing_install_script_fails_the_update() {
    let (_tmp, root) = world();
    let api = serve(
        Offer::Release {
            tag: "v99.0.0",
            sums: true,
        },
        tar_gz(&marker_script(&root, 3)),
    );
    let out = self_update(&root, &api, &[]);
    let (stdout, stderr) = texts(&out);
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(
        stderr.contains("the new horch install exited with 3"),
        "{stderr}"
    );
    assert!(!stdout.contains("updated horch"), "{stdout}");
}

#[test]
fn a_release_without_sha256sums_is_refused() {
    let (_tmp, root) = world();
    let api = serve(
        Offer::Release {
            tag: "v99.0.0",
            sums: false,
        },
        tar_gz(&marker_script(&root, 0)),
    );
    let out = self_update(&root, &api, &[]);
    let (_, stderr) = texts(&out);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "release v99.0.0 has no SHA256SUMS for target {}: {api}/release",
            env!("HORCH_TARGET")
        )),
        "{stderr}"
    );
    assert!(!root.join("marker").exists());
}
