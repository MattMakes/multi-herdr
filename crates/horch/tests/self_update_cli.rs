//! `horch self-update` against a local release server (design C3).
//!
//! Hermetic: the environment is cleared, `HORCH_RELEASE_API` points at a
//! `TcpListener` on 127.0.0.1, and every path is in a temp dir. The archive's
//! `horch` is a shell script that writes its arguments to a marker file.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

/// The archive the server offers: the one for the target `build.rs` saw.
fn archive_name() -> String {
    format!("multi-herdr-v99.0.0-{}.tar.gz", env!("HORCH_TARGET"))
}

/// An archive of the named binaries, each holding `script`.
fn tar_gz(script: &str, names: &[&str]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);
    for name in names {
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

const ALL_BINARIES: [&str; 3] = ["horch", "multi-herdr-dataset", "herdr-install"];

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Serve `/releases/latest`, `/dl/<archive>` and `/dl/SHA256SUMS` until the
/// test ends. Returns the API base URL.
fn serve(archive: Vec<u8>, sums_hex: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let name = archive_name();
    let assets: Vec<String> = [name.as_str(), "SHA256SUMS"]
        .iter()
        .map(|n| format!(r#"{{"name":"{n}","browser_download_url":"{base}/dl/{n}"}}"#))
        .collect();
    let release = format!(
        r#"{{"tag_name":"v99.0.0","html_url":"{base}/release","assets":[{}]}}"#,
        assets.join(",")
    );
    let sums = format!("{sums_hex}  {name}\n");
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
                "/releases/latest" => ("200 OK", release.clone().into_bytes()),
                "/dl/SHA256SUMS" => ("200 OK", sums.clone().into_bytes()),
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

fn self_update(root: &Path, api: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_horch"))
        .arg("self-update")
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

fn setup(names: &[&str]) -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for dir in ["home", "data", "state", "bin"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let marker = root.join("marker");
    let script = format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display());
    (tmp, root, tar_gz(&script, names))
}

#[test]
fn self_update_verifies_and_runs_the_new_horch_install() {
    let (_tmp, root, archive) = setup(&ALL_BINARIES);
    let sums = hex(&archive);
    let api = serve(archive, sums);
    std::fs::write(root.join("state/self-check.json"), "{}").unwrap();

    let out = self_update(&root, &api);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    let marker = std::fs::read_to_string(root.join("marker")).unwrap();
    assert_eq!(
        marker.trim_end(),
        format!("install --dir {}", root.join("bin").display())
    );
    assert!(stdout.contains("-> v99.0.0"), "{stdout}");
    assert!(!root.join("state/self-check.json").exists());
}

#[test]
fn self_update_refuses_a_wrong_digest_and_installs_nothing() {
    let (_tmp, root, archive) = setup(&ALL_BINARIES);
    let api = serve(archive, "0".repeat(64));

    let out = self_update(&root, &api);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("checksum mismatch"), "{stderr}");
    assert!(!root.join("marker").exists());
}

#[test]
fn self_update_refuses_an_archive_that_lacks_a_binary_and_installs_nothing() {
    let (_tmp, root, archive) = setup(&["horch"]);
    let sums = hex(&archive);
    let api = serve(archive, sums);

    let out = self_update(&root, &api);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("multi-herdr-dataset"), "{stderr}");
    assert!(stderr.contains("herdr-install"), "{stderr}");
    assert!(stderr.contains("v99.0.0"), "{stderr}");
    assert!(!root.join("marker").exists());
}
