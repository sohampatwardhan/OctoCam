# OctoCam Root SSH Key Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an admin-gated "SSH keys" page to `octocam-web` that lists the public keys authorized for root SSH login (`/root/.ssh/authorized_keys`), lets the admin revoke any key, and lets the admin authorize (add) a new public key. Revoking the *last* remaining key surfaces a lockout warning that must be confirmed.

**Architecture:** One new synchronous module (`ssh_keys`) owns all parsing, validation, fingerprinting, and privileged file IO. Reads use `sudo -n cat`; writes stage a service-user temp file and atomically replace the target with `sudo -n install` — mirroring the existing `schedule_systemctl` sudo pattern. Three new Axum handlers (page GET, add POST, revoke POST) in `main.rs`, all gated by `require_admin_login` + `setup_complete`, all running the blocking sudo calls through the existing `run_blocking` helper. One new template plus a sidebar nav entry.

**Tech Stack:** Rust, Axum 0.8, Askama templates, `std::process` via `crate::proc::run`, `sha2` + `base64` (both already in `Cargo.toml`) for OpenSSH SHA256 fingerprints. **No new crate dependencies. No install.sh / deploy / sudoers changes** (relies on the same broad `sudo -n` the service user already uses for `systemctl`).

**Background:** `octocam-web` runs as a non-root `SERVICE_USER` (e.g. `dietpi`) and reaches privileged operations via `sudo -n` (see `schedule_systemctl` in `main.rs:1104`). [advanced.html](../../../rust/octocam-web/templates/advanced.html) already directs users to `ssh root@<pi>`, so root's `/root/.ssh/authorized_keys` is the SSH target this feature manages.

---

## File Structure

- **Create** `rust/octocam-web/src/ssh_keys.rs` — parsing, validation, fingerprint, and privileged read/write of `/root/.ssh/authorized_keys`. Pure logic is unit-tested; IO goes through `crate::proc::run`.
- **Modify** `rust/octocam-web/src/main.rs` — `mod ssh_keys;`; add `SshKeysTemplate`; add handlers `ssh_keys_page` (GET `/ssh-keys`), `ssh_keys_add` (POST `/ssh-keys/add`), `ssh_keys_revoke` (POST `/ssh-keys/revoke`); register the three routes.
- **Create** `rust/octocam-web/templates/ssh_keys.html` — same shell as `advanced.html` (topbar + `_sidebar.html` include). Panel 1: key list with per-key revoke buttons + last-key confirm affordance. Panel 2: textarea + "Authorize key" add form. Renders a message banner from a query param.
- **Modify** `rust/octocam-web/templates/_sidebar.html` — add a nav link to `/ssh-keys` under the "Advanced Settings" group with `active_page == "ssh_keys"`.

---

## Design Decisions (read before starting)

1. **Target file is hardcoded to `/root/.ssh/authorized_keys`.** No other user/account is in scope (YAGNI). Path is a module constant.

2. **Privilege model — reuse the existing `sudo -n` pattern.**
   - Read: `sudo -n cat /root/.ssh/authorized_keys`. A non-zero exit *because the file does not exist* → treat as empty (no keys), not an error. A non-zero exit for any other reason (sudo denied) → surface as a read error, but the page still renders (empty list + a note) rather than 500ing.
   - Write (atomic): write full new contents to a temp file the service user owns under the state dir, then:
     - `sudo -n install -d -m 700 -o root -g root /root/.ssh` (idempotent; ensures dir exists with correct perms)
     - `sudo -n install -m 600 -o root -g root <tmp> /root/.ssh/authorized_keys` (atomic replace)
   - Remove the temp file afterward (best-effort).

3. **Parsing preserves unknown lines.** `parse_authorized_keys(&str)` returns a `Vec<AuthorizedKey>` for display, but the *rewrite* path operates on the original file's lines so comment lines (`#…`), blank lines, and option-carrying entries we don't fully model are never silently dropped. Revoke removes exactly the line(s) whose parsed key matches the target fingerprint.

4. **Fingerprint is the stable identity.** Compute the standard OpenSSH `SHA256:<unpadded-base64(sha256(raw key blob))>` in-process (decode the base64 body, `Sha256` it, `base64` STANDARD_NO_PAD the digest). Revoke targets a fingerprint, not an array index, so a concurrent edit can't revoke the wrong key.

5. **Add-key validation (defense against file/command injection).** Trim the submission. Reject if it contains any `\n`, `\r`, or NUL (blocks multi-line / multi-key injection). Split on whitespace; require the first token to be an allowlisted type (`ssh-ed25519`, `ssh-rsa`, `ecdsa-sha2-nistp256/384/521`, `sk-ssh-ed25519@openssh.com`, `sk-ecdsa-sha2-nistp256@openssh.com`) and the second token to base64-decode successfully. Optional third token (comment) is kept verbatim. Options-prefixed keys are rejected with a clear message (out of scope). Reject a key whose fingerprint already exists (idempotent, avoids dupes).

6. **Last-key lockout guard.** `ssh_keys_revoke` takes form fields `fingerprint` and optional `confirm=1`. If the target is the only authorized key and `confirm` is absent, do NOT revoke — redirect back with a warning state (`?warn=<fingerprint>`) so the template re-renders that key's revoke control as a two-step "Yes, remove my last key and end root SSH" confirm that resubmits with `confirm=1`. With `confirm=1`, or when more than one key remains, revoke proceeds.

7. **Async boundary.** All three handlers follow the existing shape: `require_admin_login` → `settings::load_settings` → `setup_complete` redirect → do privileged work inside `run_blocking(move || …)` → redirect (POST) or `render` (GET). No blocking subprocess ever runs directly on a Tokio worker (per the non-blocking-subprocess plan).

8. **User feedback via redirect + query param**, matching `matter_reset` (`/matter?saved=1`) and Wi-Fi (`?wifi_message=…`). Success/error/warn messages are URL-encoded and rendered as a banner. Full key material is never placed in a query string or logged.

---

## Tasks

### Task 1 — `ssh_keys` module: pure logic + tests
- [ ] Create `rust/octocam-web/src/ssh_keys.rs`.
- [ ] `pub struct AuthorizedKey { pub key_type: String, pub comment: String, pub fingerprint: String, pub preview: String }` (`preview` = first ~12 + last ~8 chars of the body for display).
- [ ] `pub fn fingerprint(body_b64: &str) -> Option<String>` — decode STANDARD base64 body; `Sha256`; return `SHA256:` + STANDARD_NO_PAD base64 of digest. `None` if body doesn't decode.
- [ ] `pub fn parse_authorized_keys(contents: &str) -> Vec<AuthorizedKey>` — per line: skip blank / `#`-leading; split whitespace; if first token is an allowlisted type and body decodes, build an `AuthorizedKey`; otherwise skip for display (but see rewrite note).
- [ ] `pub fn validate_new_key(input: &str) -> Result<String, String>` — trim, reject control chars, enforce type allowlist + base64 body, return the normalized single-line entry to append; `Err(msg)` otherwise.
- [ ] Unit tests: fingerprint against a known `ssh-ed25519` vector; parse multi-key file incl. comment + blank lines; validate rejects multiline, bad base64, bad type, options-prefixed, empty.

### Task 2 — `ssh_keys` module: privileged IO
- [ ] `pub fn read_raw() -> Result<String, String>` — `crate::proc::run(sudo -n cat /root/.ssh/authorized_keys, DEFAULT_TIMEOUT)`. Missing-file exit → `Ok(String::new())`; other failure → `Err`.
- [ ] `pub fn list() -> Result<Vec<AuthorizedKey>, String>` — `read_raw()` → `parse_authorized_keys`.
- [ ] `fn write_raw(contents: &str) -> Result<(), String>` — write temp file under state dir (path via env `OCTOCAM_STATE_DIR` or derive from config path dir; fall back to `std::env::temp_dir()`), then the two `sudo -n install` calls; clean up temp. Non-zero exit → `Err` with stderr summary.
- [ ] `pub fn add(input: &str) -> Result<(), String>` — validate; `read_raw`; reject duplicate fingerprint; append normalized line (ensure trailing newline hygiene); `write_raw`.
- [ ] `pub fn revoke(fingerprint: &str) -> Result<(), String>` — `read_raw`; rewrite keeping every original line whose parsed fingerprint != target (non-key lines preserved verbatim); `write_raw`.
- [ ] `pub fn count() -> Result<usize, String>` — number of parseable keys (for last-key guard). (Or return count from `list`.)

### Task 3 — Handlers + routes in `main.rs`
- [ ] Add `mod ssh_keys;`.
- [ ] `#[derive(Template)] struct SshKeysTemplate` with `page_title`, `settings`, `system: SystemView`, `active_page`, `keys: Vec<ssh_keys::AuthorizedKey>`, `message: Option<String>`, `is_error: bool`, `warn_fingerprint: Option<String>`, `read_error: bool`.
- [ ] `ssh_keys_page` (GET `/ssh-keys`) — admin+setup guard; `run_blocking(ssh_keys::list)`; on `Err`, render with empty keys + `read_error=true`; parse `?msg`/`?err`/`?warn` query into template fields.
- [ ] `ssh_keys_add` (POST `/ssh-keys/add`, form `public_key`) — admin+setup guard; `run_blocking(move || ssh_keys::add(&public_key))`; redirect `/ssh-keys?msg=…` or `?err=…`.
- [ ] `ssh_keys_revoke` (POST `/ssh-keys/revoke`, form `fingerprint`, optional `confirm`) — admin+setup guard; `run_blocking` to get count + perform guard logic: if count<=1 and no confirm → redirect `/ssh-keys?warn=<fp>`; else revoke → redirect with msg/err.
- [ ] Register the three routes in the router alongside the others.

### Task 4 — Template + nav
- [ ] Create `templates/ssh_keys.html` cloning `advanced.html`'s shell (topbar, `_sidebar.html`, `content-stack`).
- [ ] Message banner (success/error/warn) at top of content when present.
- [ ] Panel "Authorized keys": if `read_error`, show a note ("Couldn't read root's authorized_keys — check sudo access"); else if empty, show empty state; else list each key (`key_type`, `comment`, `fingerprint`, `preview`) with a `POST /ssh-keys/revoke` form carrying `fingerprint`. When `warn_fingerprint` matches a row, render the confirm variant (adds `confirm=1`, warning copy).
- [ ] Panel "Authorize a new key": `POST /ssh-keys/add` with a `<textarea name="public_key">` and submit button; helper text (paste one public key line).
- [ ] Add sidebar nav link in `_sidebar.html` under "Advanced Settings" with the key/lock icon and `active_page == "ssh_keys"`.

### Task 5 — Build + verify
- [ ] `cargo build` (host) and `cargo test` (ssh_keys unit tests pass).
- [ ] `cargo clippy` clean for new code.
- [ ] Manual reasoning check of the render + redirect paths.

---

## Hardening Addenda (plan-harden thorough, 2026-07-03)

Produced by a 3-reviewer pass (gap / code / adversarial). These **override or augment** the tasks above — apply them in place. The gap reviewer stalled; its two linchpins were verified directly: (a) no `OCTOCAM_STATE_DIR` env exists — state lives under `config_path.parent()` = `/var/lib/octocam`; (b) the service user runs broad `sudo -n` (the deploy script relies on it as `dietpi@`).

### FIX-1 (P1) — `Digest` trait import
`Sha256::digest` is a trait method. `ssh_keys.rs` must `use sha2::{Digest, Sha256};` (repo only ever used `Sha256` via `Hmac`, so no precedent).

### FIX-2 (P1) — `Engine` trait import
base64 `.decode()`/`.encode()` are `Engine` methods: `use base64::{engine::general_purpose::{STANDARD, STANDARD_NO_PAD}, Engine};` (mirrors `security.rs:1`).

### FIX-3 (P2) — `run_blocking` yields a nested Result
`run_blocking<T,F>(f) -> Result<T, AppError>`. With `list() -> Result<Vec<_>, String>` the awaited value is `Result<Result<Vec<_>, String>, AppError>`. Handle with a triple match (`Ok(Ok(keys))` / `Ok(Err(msg))` / `Err(_join)`) exactly like `scan_wifi` (main.rs:929) and `api_wifi_scan` (main.rs:1222). Both `Ok(Err)` and `Err(join)` set `read_error=true`.

### FIX-4 (P2) — Revoke handler is one `run_blocking` call returning an enum
Do count-check + guard + revoke inside a single blocking closure returning:
`enum RevokeOutcome { Warn, Revoked, Failed(&'static str) }` (or similar). Handler maps the outcome to a redirect. Avoids two `sudo cat` round-trips and matches Design Decision #7.

### FIX-5 (P3→adopt) — Template message convention
Use the repo's paired `String` + `has_x: bool` fields, NOT `Option<String>`. `SshKeysTemplate` fields: `page_title, settings, system, active_page, keys: Vec<AuthorizedKey>, message: String, has_message: bool, message_is_error: bool, warn_fingerprint: String, has_warn: bool, read_error: bool`.

### FIX-6 (P0) — Atomic write with pre-install verification (prevents lockout)
Replaces Decision 2's write path. `write_raw(new_contents, expected_key_count, allow_empty)`:
1. Stage `new_contents` to `<state_dir>/.authorized_keys.octocam.tmp` (service-user owned; `state_dir = config_path.parent()`, see FIX-12).
2. **Verify** the staged bytes: `parse_authorized_keys(staged).len() == expected_key_count`, and reject a zero-key result unless `allow_empty` (only true for a confirmed last-key revoke). Refuse to install otherwise → returns `Err`, real file untouched.
3. `sudo -n install -d -m 700 -o root -g root /root/.ssh` (idempotent).
4. `sudo -n install -m 600 -o root -g root <tmp> /root/.ssh/.authorized_keys.octocam.new` (partial writes land here, not on the live file).
5. `sudo -n mv -f /root/.ssh/.authorized_keys.octocam.new /root/.ssh/authorized_keys` — **atomic rename within /root/.ssh**. Only runs if step 4 exited 0.
6. Best-effort remove the service-user temp. Any non-zero exit → `Err`, live file intact.

### FIX-7 (P1) — Last-key guard counts keys-remaining-after-rewrite; revoke deletes all matching lines
`ssh-copy-id` can leave duplicate lines with the same fingerprint. `revoke(fp)` removes **every** line whose parsed fingerprint == `fp`. So the guard must compute the file **after** the rewrite and count remaining parseable keys — not "current count <= 1". If remaining == 0 and no `confirm=1`, redirect `?warn=<fp>` (no write). With `confirm=1`, call `write_raw(.., 0, allow_empty=true)`. This also fixes the false "last key" warning when unmodeled option-keys exist (count reflects reality).

### FIX-8 (P2) — `read_raw` fails closed
`sudo -n cat`: exit 0 → `Ok(stdout)`. Non-zero: if stderr indicates ENOENT ("No such file") → `Ok(String::new())` (empty, file absent). Any other non-zero (sudo denied, EACCES) → `Err`. `add`/`revoke` MUST propagate a read `Err` and refuse to write (never clobber an unread file). `list` maps `Err` → `read_error=true`, empty display.

### FIX-9 (P1) — Reject all control chars + Unicode line separators; reject wrapped bodies
`validate_new_key`: reject if `input.chars().any(|c| c.is_control())` (covers `\t \r \n` NUL) or contains U+2028/U+2029. The base64 body must decode with the strict `STANDARD` engine (rejects internal whitespace / line-wrapped keys). Comment token: keep only if ASCII printable, else drop it.

### FIX-10 (P1) — Length caps
Reject a submission longer than 16 KB. In `add`, reject if the resulting file would exceed 64 KB. Guards SD-card write-amplification and sshd's own authorized_keys size limits.

### FIX-11 (P1) — Enumerated status codes, no free-form text in URLs
`TraceLayer` (main.rs:395) logs the request URI to journald. Redirect only with fixed codes: `?status=added|revoked|bad_key|duplicate|too_long|options_rejected|write_failed|read_failed` and `?warn=<fp>` (a public-key fingerprint, not secret). The page GET maps the code to a canned message + `message_is_error`. Detailed stderr is logged server-side via `eprintln!`/tracing in the handler, never via the URL. Full key material never appears in a URL or log.

### FIX-12 (P2) — Temp-file location
No `OCTOCAM_STATE_DIR`. Handlers pass `state.config_path.parent()` (`/var/lib/octocam`, service-user-owned) into `add`/`revoke`/`write_raw` as `state_dir`.

### FIX-13 (P1, judgment) — Origin/Referer same-origin check on POST; no app-wide CSRF retrofit
`/ssh-keys/add` and `/ssh-keys/revoke` reject a request whose `Origin` (or, if absent, `Referer`) is present **and** its host does not match the request `Host` header. If neither header is present, allow (matches `SameSite=Lax`, which already blocks textbook cross-site POST-form CSRF). This is contained defense-in-depth for a root-key surface. **Explicitly out of scope:** a full per-session CSRF token and cookie `Secure` flag — both are pre-existing app-wide gaps (the whole app relies on `SameSite=Lax` over plain HTTP) and should be addressed app-wide, not bolted onto this one feature.

---

## Out of Scope (YAGNI)
- Editing a key in place (revoke + re-add instead).
- Managing any account other than root.
- Rotating host keys, sshd config, or password auth toggles.
- A dedicated sudoers/polkit least-privilege helper (a broader hardening effort that should also cover `systemctl`, tracked separately if desired).
