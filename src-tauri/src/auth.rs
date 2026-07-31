//! Account sign-in for the desktop client.
//!
//! OAuth via **Supabase Auth** using Authorization Code + **PKCE** with a
//! **`conva://` deep-link return** — no client secret ships in the binary; the
//! provider secret lives only in Supabase. The system browser handles the IdP,
//! so we never touch the user's Google/LinkedIn/Facebook credentials.
//!
//! The flow is two-phase and event-driven (the same shape the mobile companion
//! will use — deep links are the one return path that works on every platform):
//! 1. [`begin_sign_in`] stores a pending PKCE verifier and opens the browser at
//!    Supabase's `/authorize`, then returns immediately.
//! 2. The browser lands on the redirect page, which hands control back via
//!    `conva://auth/callback?code=…`; the shell's deep-link handler calls
//!    [`complete_sign_in`], which exchanges the code and emits `AUTH_CHANGED`.
//!
//! Tokens live in the OS keyring (the same vault as provider API keys, service
//! `conva`); non-secret session metadata (email, expiry) is cached in app-data
//! so the UI can show "signed in as…" offline. Design: `conva_core`'s
//! `docs/platform/01-auth.md`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// The project's base URL. The anon ("publishable") key is public and safe to
// embed; fill `DEFAULT_ANON_KEY` from Supabase → Project Settings → API → anon
// public. Both are overridable via env for dev / a second project.
const DEFAULT_SUPABASE_URL: &str = "https://hbxftjyooblxiiapaeei.supabase.co";
const DEFAULT_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImhieGZ0anlvb2JseGlpYXBhZWVpIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODUyNTQ3MzksImV4cCI6MjEwMDgzMDczOX0.KkvrtUOubjv8DUym7Qj_W_YyYezkVtueKdg9LyQGqQU";

/// Where the browser lands after Supabase finishes OAuth: the hosted, branded
/// page `https://getconva.com/callback` (built in `conva_web` as
/// `callback.html`), which shows a "returning you to conva" finish and forwards
/// the `?code` into the app via the `conva://` deep link — the Zoom/Slack-style
/// close, with no local IPs on screen. The bare `conva://auth/callback` deep
/// link is still fully supported as the fallback — set
/// `CONVA_AUTH_REDIRECT_URL=conva://auth/callback` to skip the hosted page (the
/// browser then shows the OS's "Open conva?" prompt instead of a branded page).
/// Both are in the Supabase redirect allow-list (`https://getconva.com/**`,
/// `conva://**`).
///
/// NB: keep this pointed at the hosted page only while that page is deployed to
/// production — if `getconva.com/callback` 404s to the site homepage, sign-in
/// silently breaks (the homepage never forwards to `conva://`).
const DEFAULT_AUTH_REDIRECT_URL: &str = "https://getconva.com/callback";

/// The deep-link prefix the app answers for sign-in completions — the shell
/// routes any `conva://auth/…` URL to [`complete_sign_in`].
pub const AUTH_DEEP_LINK_PREFIX: &str = "conva://auth";

fn auth_redirect_url() -> String {
    std::env::var("CONVA_AUTH_REDIRECT_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_AUTH_REDIRECT_URL.to_string())
}

/// The half-open sign-in: the PKCE verifier waiting for the browser to
/// deep-link back with the matching code. One at a time — starting a new
/// sign-in replaces (invalidates) the previous pending one.
struct PendingSignIn {
    verifier: String,
    started: Instant,
}

static PENDING_SIGN_IN: Mutex<Option<PendingSignIn>> = Mutex::new(None);

/// A sign-in left unanswered this long is dead (browser closed, consent
/// abandoned) — a late deep link must not complete it.
const PENDING_SIGN_IN_TTL: Duration = Duration::from_secs(10 * 60);

/// The short-lived access JWT, in-process only. It does NOT go in the OS
/// keyring: Windows Credential Manager caps a credential blob at 2560 bytes
/// counted in UTF-16 (`CRED_MAX_CREDENTIAL_BLOB_SIZE`), and Supabase access
/// tokens overflow it ("Attribute 'password encoded as UTF-16' is longer than
/// platform limit"). It expires in ~1 h and is re-minted from the (small,
/// keyring-stored) refresh token anyway — memory is the right home per the
/// design doc (`01-auth.md`, desktop token storage).
static ACCESS_TOKEN: Mutex<Option<String>> = Mutex::new(None);

const KEYRING_SERVICE: &str = "conva";
const KR_REFRESH: &str = "auth-refresh-token";
const KR_ACCESS: &str = "auth-access-token";
const META_FILE: &str = "auth.json";

// Backend resolution precedence (first non-empty wins):
//   1. runtime env  CONVA_SUPABASE_URL  — local `tauri dev` against any project
//   2. compile-time env (option_env!)   — baked by CI (dev-build.yml sets it from
//      the decrypted .env.dev), so a DISTRIBUTED dev installer points at
//      conva-core-dev even though the tester has no env vars set
//   3. DEFAULT_*                          — live prod (conva-core)
fn supabase_url() -> String {
    std::env::var("CONVA_SUPABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            option_env!("CONVA_SUPABASE_URL")
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| DEFAULT_SUPABASE_URL.to_string())
}

fn anon_key() -> String {
    std::env::var("CONVA_SUPABASE_ANON_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            option_env!("CONVA_SUPABASE_ANON_KEY")
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| DEFAULT_ANON_KEY.to_string())
}

/// Command-facing sign-in state (mirrored in `src/lib/ipc.ts` as `AuthStatus`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub expires_at_unix: Option<i64>,
    /// False when no anon key is compiled/env-configured — the UI can then
    /// explain sign-in is unavailable instead of failing opaquely.
    pub configured: bool,
}

/// Payload of the `conva://auth-changed` event (event name in
/// `conva-core::ipc::events`, TS mirror in `src/lib/ipc.ts`): the result of an
/// OAuth sign-in finishing out-of-band via the deep link. Exactly one of
/// `status` / `error` is set.
#[derive(Debug, Clone, Serialize)]
pub struct AuthChangedEvent {
    pub status: Option<AuthStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionMeta {
    user_id: Option<String>,
    email: Option<String>,
    expires_at_unix: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: Option<i64>,
    expires_at: Option<i64>,
    user: Option<UserObj>,
}

#[derive(Debug, Deserialize)]
struct UserObj {
    id: String,
    email: Option<String>,
}

// ------------------------------------------------------------------- keyring

fn kr(user: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, user).map_err(|e| e.to_string())
}

fn kr_set(user: &str, val: &str) -> Result<(), String> {
    kr(user)?.set_password(val).map_err(|e| e.to_string())
}

fn kr_get(user: &str) -> Result<Option<String>, String> {
    match kr(user)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn kr_del(user: &str) -> Result<(), String> {
    match kr(user)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------- PKCE

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 32 bytes of OS randomness, base64url — the PKCE `code_verifier`.
fn random_token() -> String {
    let mut buf = [0u8; 32];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut buf);
    b64url(&buf)
}

/// S256 challenge = base64url(sha256(verifier)).
fn challenge_of(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

// -------------------------------------------------------------- browser open

/// Open a URL in the user's default browser. Used for the OAuth authorize URL
/// and for external links (e.g. the website's password-reset page).
pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // Do NOT route the URL through `cmd /C start` (cmd splits on `&` and does
        // `%…%` env expansion, truncating/mangling an OAuth URL like
        // `…/authorize?provider=google&redirect_to=http%3A%2F%2F127.0.0.1…&code_challenge=…`
        // at the first `&` — dropping `redirect_to`, so Supabase falls back to the
        // Site URL/conva-app.com, and `code_challenge`, so it returns an implicit
        // `#access_token`; that was the real "Google sign-in bounces to
        // conva-app.com" bug). Also NOT `explorer.exe <url>` — on some setups it
        // opens a File Explorer window instead of the browser.
        // `rundll32 url.dll,FileProtocolHandler <url>` is the documented Windows
        // call that hands the URL — `&`, percent-encoding and all — to the default
        // browser's protocol handler as a single argument.
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

// ------------------------------------------------------------ query parsing

/// Extract the query pairs from a URL or request target — works for both
/// `https://…/callback?code=…` and `conva://auth/callback?code=…` (splits at
/// the first `?`).
fn parse_query(target: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some((_, q)) = target.split_once('?') {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                let val = urlencoding::decode(v)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| v.to_string());
                map.insert(k.to_string(), val);
            }
        }
    }
    map
}

// ------------------------------------------------------------- token exchange

/// Turn a `ureq` failure into a human-readable message. On an HTTP error status
/// Supabase returns a JSON body (`error_description` / `msg` / `message`); pull
/// that out so the UI can show "Invalid login credentials" rather than "400".
fn friendly_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("error_description")
                        .or_else(|| v.get("msg"))
                        .or_else(|| v.get("message"))
                        .and_then(|m| m.as_str().map(str::to_string))
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("request failed ({code})"))
        }
        ureq::Error::Transport(t) => t.to_string(),
    }
}

fn exchange_code(
    base: &str,
    key: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, String> {
    let url = format!("{base}/auth/v1/token?grant_type=pkce");
    ureq::post(&url)
        .set("apikey", key)
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!({ "auth_code": code, "code_verifier": verifier }))
        .map_err(|e| e.to_string())?
        .into_json::<TokenResponse>()
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- persistence

fn meta_path(auth_dir: &Path) -> std::path::PathBuf {
    auth_dir.join(META_FILE)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn persist(t: &TokenResponse, auth_dir: &Path) -> Result<(), String> {
    // Only the (small) refresh token goes in the keyring; the access JWT is
    // held in memory — it's short-lived, re-minted from the refresh token, and
    // too large for Windows Credential Manager (see ACCESS_TOKEN).
    kr_set(KR_REFRESH, &t.refresh_token)?;
    *ACCESS_TOKEN.lock().expect("access token lock") = Some(t.access_token.clone());
    let expires_at = t
        .expires_at
        .or_else(|| t.expires_in.map(|s| now_unix() + s));
    let meta = SessionMeta {
        user_id: t.user.as_ref().map(|u| u.id.clone()),
        email: t.user.as_ref().and_then(|u| u.email.clone()),
        expires_at_unix: expires_at,
    };
    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    std::fs::write(meta_path(auth_dir), json).map_err(|e| e.to_string())
}

fn read_meta(auth_dir: &Path) -> Option<SessionMeta> {
    let raw = std::fs::read_to_string(meta_path(auth_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

// --------------------------------------------------------------------- public

/// Phase 1 of the interactive sign-in: stash a pending PKCE verifier and open
/// the system browser at Supabase's `/authorize`. Returns immediately — the
/// result arrives when the browser deep-links back (`conva://auth/callback`)
/// and the shell calls [`complete_sign_in`]. Call from `spawn_blocking` (the
/// browser launch spawns a process).
pub fn begin_sign_in(provider: &str) -> Result<(), String> {
    let base = supabase_url();
    if anon_key().is_empty() {
        return Err("supabase_not_configured".to_string());
    }

    let verifier = random_token();
    let challenge = challenge_of(&verifier);
    let redirect = auth_redirect_url();

    // Do NOT pass a client `state`. Supabase's `/authorize` manages the OAuth
    // state itself: it mints a state that keys the server-side *flow_state*
    // where our `redirect_to` and PKCE challenge are stored, then round-trips
    // it through the IdP. Supplying our own state overwrites that, so on the
    // IdP callback Supabase can't resolve the flow_state — it drops
    // `redirect_to` (falling back to the Site URL) AND drops PKCE (returning
    // an implicit `#access_token`). PKCE (the `code_verifier` only this
    // process holds) provides the CSRF / code-injection protection instead.
    let authorize = format!(
        "{base}/auth/v1/authorize?provider={provider}&redirect_to={redirect}\
&code_challenge={challenge}&code_challenge_method=s256",
        redirect = urlencoding::encode(&redirect),
    );

    *PENDING_SIGN_IN.lock().expect("auth pending lock") = Some(PendingSignIn {
        verifier,
        started: Instant::now(),
    });
    eprintln!(
        "[auth] sign-in started (provider={provider}, redirect_to={redirect}); \
         waiting for the browser to deep-link back"
    );
    open_browser(&authorize)
}

/// Abandon the pending sign-in (UI "cancel" — e.g. the user closed the
/// browser). A deep link arriving afterwards is ignored as stale.
pub fn cancel_sign_in() {
    *PENDING_SIGN_IN.lock().expect("auth pending lock") = None;
}

/// Phase 2: complete a sign-in from a `conva://auth/callback?code=…` deep
/// link. Exchanges the code against the pending verifier, persists the
/// session, and returns the new status. `Ok(None)` means the link was stale or
/// duplicate (no sign-in pending) — callers should ignore it silently rather
/// than surface an error. Blocking (token exchange) — call from
/// `spawn_blocking`.
pub fn complete_sign_in(url: &str, auth_dir: &Path) -> Result<Option<AuthStatus>, String> {
    let Some(pending) = PENDING_SIGN_IN.lock().expect("auth pending lock").take() else {
        return Ok(None);
    };
    if pending.started.elapsed() > PENDING_SIGN_IN_TTL {
        return Err("sign-in expired — try again".to_string());
    }

    let params = parse_query(url);
    if let Some(err) = params.get("error") {
        let desc = params.get("error_description").cloned().unwrap_or_default();
        return Err(format!("oauth_error: {err} {desc}").trim().to_string());
    }
    let code = params
        .get("code")
        .cloned()
        .ok_or_else(|| "no_code".to_string())?;

    let tokens = exchange_code(&supabase_url(), &anon_key(), &code, &pending.verifier)?;
    persist(&tokens, auth_dir)?;
    Ok(Some(status(auth_dir)))
}

/// Sign in with an email + password (Supabase `grant_type=password`). Blocking —
/// call from `spawn_blocking`. The account is the *same* identity used for
/// Google / the website, so a user who signed up either way can sign in here.
pub fn sign_in_password(
    email: &str,
    password: &str,
    auth_dir: &Path,
) -> Result<AuthStatus, String> {
    let base = supabase_url();
    let key = anon_key();
    if key.is_empty() {
        return Err("supabase_not_configured".to_string());
    }
    let url = format!("{base}/auth/v1/token?grant_type=password");
    let tokens = ureq::post(&url)
        .set("apikey", &key)
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!({ "email": email, "password": password }))
        .map_err(friendly_err)?
        .into_json::<TokenResponse>()
        .map_err(|e| e.to_string())?;
    persist(&tokens, auth_dir)?;
    Ok(status(auth_dir))
}

/// Create an account with an email + password (Supabase `/signup`). If the
/// project requires email confirmation, no session is returned yet — surface
/// `email_confirmation_required` so the UI can tell the user to confirm, then
/// sign in. Blocking — call from `spawn_blocking`.
pub fn sign_up_password(
    email: &str,
    password: &str,
    auth_dir: &Path,
) -> Result<AuthStatus, String> {
    let base = supabase_url();
    let key = anon_key();
    if key.is_empty() {
        return Err("supabase_not_configured".to_string());
    }
    let url = format!("{base}/auth/v1/signup");
    let val: serde_json::Value = ureq::post(&url)
        .set("apikey", &key)
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!({ "email": email, "password": password }))
        .map_err(friendly_err)?
        .into_json()
        .map_err(|e| e.to_string())?;

    // A session (access_token) is present only when email confirmation is off.
    if val.get("access_token").and_then(|v| v.as_str()).is_some() {
        let tokens: TokenResponse = serde_json::from_value(val).map_err(|e| e.to_string())?;
        persist(&tokens, auth_dir)?;
        Ok(status(auth_dir))
    } else {
        Err("email_confirmation_required".to_string())
    }
}

/// Renew the access/refresh pair from the stored refresh token. Wired into the
/// entitlement/API layer in M1 (see conva_core `docs/platform/09-implementation-plan.md`);
/// allow(dead_code) until then so CI's `-D warnings` stays green.
#[allow(dead_code)]
pub fn refresh(auth_dir: &Path) -> Result<AuthStatus, String> {
    let base = supabase_url();
    let key = anon_key();
    if key.is_empty() {
        return Err("supabase_not_configured".to_string());
    }
    let rt = kr_get(KR_REFRESH)?.ok_or_else(|| "not_signed_in".to_string())?;
    let url = format!("{base}/auth/v1/token?grant_type=refresh_token");
    let tokens = ureq::post(&url)
        .set("apikey", &key)
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!({ "refresh_token": rt }))
        .map_err(|e| e.to_string())?
        .into_json::<TokenResponse>()
        .map_err(|e| e.to_string())?;
    persist(&tokens, auth_dir)?;
    Ok(status(auth_dir))
}

/// Non-secret, offline snapshot of the session.
pub fn status(auth_dir: &Path) -> AuthStatus {
    let configured = !anon_key().is_empty();
    let has_refresh = kr_get(KR_REFRESH).ok().flatten().is_some();
    let meta = read_meta(auth_dir);
    AuthStatus {
        signed_in: has_refresh && meta.is_some(),
        email: meta.as_ref().and_then(|m| m.email.clone()),
        user_id: meta.as_ref().and_then(|m| m.user_id.clone()),
        expires_at_unix: meta.as_ref().and_then(|m| m.expires_at_unix),
        configured,
    }
}

/// Revoke server-side (best-effort) and clear all local tokens + metadata.
pub fn sign_out(auth_dir: &Path) -> Result<(), String> {
    let key = anon_key();
    let access = ACCESS_TOKEN.lock().expect("access token lock").take();
    if let Some(access) = access {
        if !key.is_empty() {
            let url = format!("{}/auth/v1/logout", supabase_url());
            let _ = ureq::post(&url)
                .set("apikey", &key)
                .set("Authorization", &format!("Bearer {access}"))
                .call();
        }
    }
    let _ = kr_del(KR_REFRESH);
    // KR_ACCESS is no longer written (the JWT lives in memory now), but older
    // builds may have left one — delete it so no oversized blob lingers.
    let _ = kr_del(KR_ACCESS);
    let _ = std::fs::remove_file(meta_path(auth_dir));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_of(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn parse_query_decodes_pairs() {
        let q = parse_query("/callback?code=abc%20123&state=xyz");
        assert_eq!(q.get("code").map(String::as_str), Some("abc 123"));
        assert_eq!(q.get("state").map(String::as_str), Some("xyz"));
    }

    #[test]
    fn parse_query_handles_deep_link_urls() {
        let q = parse_query("conva://auth/callback?code=abc-123&error_description=denied%21");
        assert_eq!(q.get("code").map(String::as_str), Some("abc-123"));
        assert_eq!(
            q.get("error_description").map(String::as_str),
            Some("denied!")
        );
    }

    #[test]
    fn stale_deep_link_without_pending_sign_in_is_ignored() {
        cancel_sign_in();
        let r = complete_sign_in("conva://auth/callback?code=abc", Path::new("."));
        assert!(matches!(r, Ok(None)));
    }

    #[test]
    fn random_token_is_urlsafe_and_unique() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/') && !a.contains('='));
    }
}
