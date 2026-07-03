//! View, add, and revoke the SSH public keys authorized for root login
//! (`/root/.ssh/authorized_keys`).
//!
//! `octocam-web` runs as a non-root service user, so every access to root's
//! key file goes through `sudo -n` — the same mechanism `schedule_systemctl`
//! uses for `reboot`/`poweroff`. Reads use `sudo -n cat`; writes stage a
//! service-user temp file, verify it, `install` it into `/root/.ssh` under a
//! temp name, then atomically `mv` it over the live file so a partial write can
//! never truncate `authorized_keys` and lock everyone out.

use crate::proc;
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

const SSH_DIR: &str = "/root/.ssh";
const AUTHORIZED_KEYS_PATH: &str = "/root/.ssh/authorized_keys";
const STAGE_DEST: &str = "/root/.ssh/.authorized_keys.octocam.new";
const TEMP_NAME: &str = ".authorized_keys.octocam.tmp";

/// Reject a single submission longer than this. A real ed25519 key is ~100
/// bytes and RSA-4096 ~750; 16 KiB is generous while blocking write-amplification.
const MAX_KEY_LEN: usize = 16 * 1024;
/// Refuse to write an `authorized_keys` larger than this (sshd itself caps it,
/// and we don't want to grind the SD card).
const MAX_FILE_LEN: usize = 64 * 1024;

/// Key types we model. Anything else (including options-prefixed entries) is
/// skipped for display and rejected on add.
const KEY_TYPES: &[&str] = &[
    "ssh-ed25519",
    "ssh-rsa",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];

/// A parsed public key line, ready for display.
#[derive(Clone, Debug)]
pub struct AuthorizedKey {
    pub key_type: String,
    pub comment: String,
    pub fingerprint: String,
    pub preview: String,
}

/// Coarse, enumerated failure. The handler maps `code()` to a canned message
/// and a redirect status param — detailed causes are logged server-side, never
/// placed in a URL (which `TraceLayer` would write to journald).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    BadKey,
    TooLong,
    Duplicate,
    ReadFailed,
    WriteFailed,
}

impl KeyError {
    pub fn code(self) -> &'static str {
        match self {
            KeyError::BadKey => "bad_key",
            KeyError::TooLong => "too_long",
            KeyError::Duplicate => "duplicate",
            KeyError::ReadFailed => "read_failed",
            KeyError::WriteFailed => "write_failed",
        }
    }
}

/// Result of a revoke attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeOutcome {
    /// Would remove the last key and the caller did not confirm — no write done.
    Warn,
    /// Key removed (or was already absent).
    Revoked,
}

/// Standard OpenSSH `SHA256:<unpadded-base64(sha256(blob))>` fingerprint of a
/// base64 key body. `None` if the body is not valid standard base64.
pub fn fingerprint(body_b64: &str) -> Option<String> {
    let raw = STANDARD.decode(body_b64.as_bytes()).ok()?;
    let digest = Sha256::digest(&raw);
    Some(format!("SHA256:{}", STANDARD_NO_PAD.encode(digest)))
}

fn preview_body(body: &str) -> String {
    // body is base64 (ASCII), so byte slicing is safe.
    if body.len() <= 24 {
        body.to_string()
    } else {
        format!("{}…{}", &body[..12], &body[body.len() - 8..])
    }
}

/// Parse one line into an `AuthorizedKey`, or `None` for blank/comment/unknown
/// lines. Unknown lines (including options-prefixed keys we don't model) are
/// deliberately skipped for display but preserved on rewrite by `revoke`.
fn parse_line(line: &str) -> Option<AuthorizedKey> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let key_type = parts.next()?;
    if !KEY_TYPES.contains(&key_type) {
        return None;
    }
    let body = parts.next()?;
    let fingerprint = fingerprint(body)?;
    let comment = parts.collect::<Vec<_>>().join(" ");
    Some(AuthorizedKey {
        key_type: key_type.to_string(),
        comment,
        fingerprint,
        preview: preview_body(body),
    })
}

/// Parse the whole file into the keys we can display.
pub fn parse_authorized_keys(contents: &str) -> Vec<AuthorizedKey> {
    contents.lines().filter_map(parse_line).collect()
}

/// Validate a pasted public key and return the normalized single-line entry to
/// append. Rejects multi-line/control-char input (blocks line injection),
/// oversized input, unknown types, options-prefixed keys, and non-base64 bodies.
pub fn validate_new_key(input: &str) -> Result<String, KeyError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(KeyError::BadKey);
    }
    if trimmed.len() > MAX_KEY_LEN {
        return Err(KeyError::TooLong);
    }
    // Reject any control char (\t \r \n NUL, C0/C1) and Unicode line separators,
    // so a comment can never smuggle a newline (a second authorized line) or a
    // terminal escape that corrupts logs / an admin's `cat`.
    if trimmed
        .chars()
        .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
    {
        return Err(KeyError::BadKey);
    }
    let mut parts = trimmed.split_whitespace();
    let key_type = parts.next().ok_or(KeyError::BadKey)?;
    if !KEY_TYPES.contains(&key_type) {
        return Err(KeyError::BadKey);
    }
    let body = parts.next().ok_or(KeyError::BadKey)?;
    // Strict decode rejects internal whitespace / line-wrapped bodies too.
    if fingerprint(body).is_none() {
        return Err(KeyError::BadKey);
    }
    let comment = parts.collect::<Vec<_>>().join(" ");
    let normalized = if comment.is_empty() {
        format!("{key_type} {body}")
    } else if comment.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        format!("{key_type} {body} {comment}")
    } else {
        // Drop a non-ASCII comment rather than store it.
        format!("{key_type} {body}")
    };
    Ok(normalized)
}

/// Read `authorized_keys` via `sudo -n cat`. Fails **closed**: a missing file is
/// `Ok("")`, but any other non-zero exit (sudo denied, EACCES) is `Err` so we
/// never rewrite the file based on an uncertain read.
pub fn read_raw() -> Result<String, KeyError> {
    let output = proc::run(
        Command::new("sudo").args(["-n", "cat", AUTHORIZED_KEYS_PATH]),
        proc::DEFAULT_TIMEOUT,
    )
    .map_err(|error| {
        eprintln!("ssh_keys: cat failed to run: {error}");
        KeyError::ReadFailed
    })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("No such file") {
        return Ok(String::new());
    }
    eprintln!("ssh_keys: read failed: {}", stderr.trim());
    Err(KeyError::ReadFailed)
}

/// Parsed keys for display; surfaces a read failure so the page can show a note.
pub fn list() -> Result<Vec<AuthorizedKey>, KeyError> {
    Ok(parse_authorized_keys(&read_raw()?))
}

fn sudo(args: &[&str]) -> Result<(), KeyError> {
    let mut full = vec!["-n"];
    full.extend_from_slice(args);
    let output = proc::run(
        Command::new("sudo").args(&full),
        proc::SERVICE_TIMEOUT,
    )
    .map_err(|error| {
        eprintln!("ssh_keys: `sudo {}` failed to run: {error}", args.join(" "));
        KeyError::WriteFailed
    })?;
    if output.status.success() {
        Ok(())
    } else {
        eprintln!(
            "ssh_keys: `sudo {}` exited nonzero: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Err(KeyError::WriteFailed)
    }
}

/// Atomically replace `authorized_keys` with `contents`.
///
/// `expected_keys` is the number of parseable keys the caller believes it wrote;
/// we re-parse the staged bytes and refuse to install if it disagrees, or if the
/// result is empty and `allow_empty` is false. This makes a truncated/garbled
/// staging (e.g. ENOSPC) or a logic bug fail *before* touching the live file.
fn write_raw(
    state_dir: &Path,
    contents: &str,
    expected_keys: usize,
    allow_empty: bool,
) -> Result<(), KeyError> {
    if contents.len() > MAX_FILE_LEN {
        eprintln!("ssh_keys: refusing to write oversized authorized_keys");
        return Err(KeyError::TooLong);
    }
    let parsed = parse_authorized_keys(contents).len();
    if parsed != expected_keys {
        eprintln!("ssh_keys: staged key count {parsed} != expected {expected_keys}; aborting");
        return Err(KeyError::WriteFailed);
    }
    if parsed == 0 && !allow_empty {
        eprintln!("ssh_keys: refusing to write zero keys without confirmation");
        return Err(KeyError::WriteFailed);
    }

    let tmp = state_dir.join(TEMP_NAME);
    let tmp_str = tmp.to_str().ok_or(KeyError::WriteFailed)?;
    std::fs::write(&tmp, contents).map_err(|error| {
        eprintln!("ssh_keys: failed to stage temp file: {error}");
        KeyError::WriteFailed
    })?;

    let result = (|| {
        sudo(&["install", "-d", "-m", "700", "-o", "root", "-g", "root", SSH_DIR])?;
        // Partial writes land on STAGE_DEST, never the live file.
        sudo(&[
            "install", "-m", "600", "-o", "root", "-g", "root", tmp_str, STAGE_DEST,
        ])?;
        // Atomic rename within /root/.ssh — only runs if the install above succeeded.
        sudo(&["mv", "-f", STAGE_DEST, AUTHORIZED_KEYS_PATH])
    })();

    let _ = std::fs::remove_file(&tmp);
    result
}

/// Validate and append a new public key. Reads the current file (fail-closed),
/// rejects a duplicate fingerprint, then atomically rewrites.
pub fn add(state_dir: &Path, input: &str) -> Result<(), KeyError> {
    let line = validate_new_key(input)?;
    let raw = read_raw()?;
    let existing = parse_authorized_keys(&raw);

    let body = line.split_whitespace().nth(1).ok_or(KeyError::BadKey)?;
    let new_fp = fingerprint(body).ok_or(KeyError::BadKey)?;
    if existing.iter().any(|key| key.fingerprint == new_fp) {
        return Err(KeyError::Duplicate);
    }

    let mut contents = raw;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&line);
    contents.push('\n');
    if contents.len() > MAX_FILE_LEN {
        return Err(KeyError::TooLong);
    }
    write_raw(state_dir, &contents, existing.len() + 1, false)
}

/// Remove every line whose parsed fingerprint matches `target_fp`, preserving
/// comment/blank/unmodeled lines verbatim. If that would leave zero keys and
/// `confirm` is false, returns `Warn` without writing (last-key lockout guard).
pub fn revoke(state_dir: &Path, target_fp: &str, confirm: bool) -> Result<RevokeOutcome, KeyError> {
    let raw = read_raw()?;
    let mut kept = String::new();
    let mut removed = false;
    for line in raw.lines() {
        let is_target = parse_line(line)
            .map(|key| key.fingerprint == target_fp)
            .unwrap_or(false);
        if is_target {
            removed = true;
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    if !removed {
        // Fingerprint already gone; nothing to do.
        return Ok(RevokeOutcome::Revoked);
    }
    // Count what would actually remain — duplicate-fingerprint lines are all
    // removed together, so this can be lower than "current count minus one".
    let remaining = parse_authorized_keys(&kept).len();
    if remaining == 0 && !confirm {
        return Ok(RevokeOutcome::Warn);
    }
    write_raw(state_dir, &kept, remaining, remaining == 0)?;
    Ok(RevokeOutcome::Revoked)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real vector from `ssh-keygen -t ed25519` + `ssh-keygen -lf`.
    const ED25519_BODY: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIKlEtW3JnIreaTHkJdCpJfZFQ7fIceYo623IApq+6H7P";
    const ED25519_FPR: &str = "SHA256:i/839lxlU5LeiEz6E+/hVQWQCv2nxXreNmKFxY4+VrM";

    #[test]
    fn fingerprint_matches_ssh_keygen() {
        assert_eq!(fingerprint(ED25519_BODY).as_deref(), Some(ED25519_FPR));
    }

    #[test]
    fn fingerprint_rejects_non_base64() {
        assert_eq!(fingerprint("not valid base64!!!"), None);
    }

    #[test]
    fn parses_keys_skipping_blank_and_comment_lines() {
        let file = format!(
            "# a comment\n\nssh-ed25519 {ED25519_BODY} alice@laptop\n\n",
        );
        let keys = parse_authorized_keys(&file);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
        assert_eq!(keys[0].comment, "alice@laptop");
        assert_eq!(keys[0].fingerprint, ED25519_FPR);
    }

    #[test]
    fn parse_skips_options_prefixed_and_unknown() {
        let file = format!(
            "no-pty ssh-ed25519 {ED25519_BODY} restricted\ngibberish here\n",
        );
        // The options-prefixed and gibberish lines are not displayed...
        assert_eq!(parse_authorized_keys(&file).len(), 0);
    }

    #[test]
    fn validate_accepts_plain_key_with_comment() {
        let line = validate_new_key(&format!("  ssh-ed25519 {ED25519_BODY} me@host \n")).unwrap();
        assert_eq!(line, format!("ssh-ed25519 {ED25519_BODY} me@host"));
    }

    #[test]
    fn validate_rejects_embedded_newline() {
        let injected = format!("ssh-ed25519 {ED25519_BODY} me\nssh-ed25519 {ED25519_BODY} evil");
        assert_eq!(validate_new_key(&injected), Err(KeyError::BadKey));
    }

    #[test]
    fn validate_rejects_tab_and_cr() {
        assert_eq!(
            validate_new_key(&format!("ssh-ed25519 {ED25519_BODY}\tcomment")),
            Err(KeyError::BadKey)
        );
        assert_eq!(
            validate_new_key(&format!("ssh-ed25519 {ED25519_BODY} c\rd")),
            Err(KeyError::BadKey)
        );
    }

    #[test]
    fn validate_rejects_bad_type_and_options_prefix() {
        assert_eq!(
            validate_new_key(&format!("ssh-dss {ED25519_BODY}")),
            Err(KeyError::BadKey)
        );
        assert_eq!(
            validate_new_key(&format!("no-pty ssh-ed25519 {ED25519_BODY}")),
            Err(KeyError::BadKey)
        );
    }

    #[test]
    fn validate_rejects_bad_base64_body() {
        assert_eq!(
            validate_new_key("ssh-ed25519 this-is-not-base64!!!"),
            Err(KeyError::BadKey)
        );
    }

    #[test]
    fn validate_rejects_oversized() {
        let huge = format!("ssh-ed25519 {}", "A".repeat(MAX_KEY_LEN + 1));
        assert_eq!(validate_new_key(&huge), Err(KeyError::TooLong));
    }

    #[test]
    fn validate_rejects_empty() {
        assert_eq!(validate_new_key("   "), Err(KeyError::BadKey));
    }
}
