//! HTTP fetching and digest verification.
//!
//! Uses ureq with rustls, so there is no OpenSSL to find at build or run time on
//! either platform - the point of doing this in Rust rather than shipping a shell
//! script plus curl.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// Release binaries are around 20MB; the zip is smaller. Leave generous headroom
/// but stay bounded.
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// Fetch a small text resource, such as a manifest.
pub fn fetch_text(url: &str) -> Result<String> {
    ureq::get(url)
        .call()
        .with_context(|| format!("cannot reach {url}"))?
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {url}"))
}

/// Download a release asset into memory.
pub fn download(url: &str) -> Result<Vec<u8>> {
    let bytes = ureq::get(url)
        .call()
        .with_context(|| format!("download failed from {url}"))?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .with_context(|| format!("reading the download from {url}"))?;
    if bytes.is_empty() {
        bail!("download from {url} was empty");
    }
    Ok(bytes)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Check a download against the digest the manifest published.
pub fn verify_digest(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        bail!(
            "checksum mismatch: manifest says {expected}, download hashes to {actual}. \
             Refusing to install."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_a_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn verify_digest_accepts_either_case_and_stray_whitespace() {
        let bytes = b"abc";
        let digest = sha256_hex(bytes);
        assert!(verify_digest(bytes, &digest).is_ok());
        assert!(verify_digest(bytes, &digest.to_uppercase()).is_ok());
        assert!(verify_digest(bytes, &format!("  {digest}\n")).is_ok());
    }

    /// A tampered or truncated download must be rejected, and the error must show
    /// both digests so it is diagnosable.
    #[test]
    fn verify_digest_rejects_a_mismatch() {
        let err = verify_digest(b"abc", &sha256_hex(b"abd")).unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(err.contains("Refusing to install"), "{err}");
    }
}
