/**
 * Web auth for the browser build (roadmap 1.2) — the real implementation behind
 * `WebBackend.auth`.
 *
 * Design: the session IS conva_web's session. `conva_web/scripts/auth.js`
 * (the getconva.com login page) writes a `conva.session` record to
 * localStorage; served same-origin (getconva.com/app), this module reads the
 * exact same record — login on the site is login in the app, with no second
 * auth stack. Email/password calls Supabase GoTrue directly with the same raw
 * REST shape auth.js uses; OAuth redirects to the shared login page (Layer 2)
 * rather than reimplementing PKCE here.
 *
 * Backend selection mirrors src-tauri/src/auth.rs: CONVA_SUPABASE_* from the
 * build env (vite `envPrefix` exposes them, so the same .env.dev/.env.prod
 * files drive desktop AND web builds) → live-prod defaults.
 */

import type { AuthStatus } from "@/lib/ipc";

// Same record `conva_web/scripts/auth.js` reads/writes — do not change shape
// without changing it there in the same commit.
const SESSION_KEY = "conva.session";

// Prod (conva-core) — same defaults baked into src-tauri/src/auth.rs.
const DEFAULT_SUPABASE_URL = "https://hbxftjyooblxiiapaeei.supabase.co";
const DEFAULT_ANON_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImhieGZ0anlvb2JseGlpYXBhZWVpIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODUyNTQ3MzksImV4cCI6MjEwMDgzMDczOX0.KkvrtUOubjv8DUym7Qj_W_YyYezkVtueKdg9LyQGqQU";

const SUPABASE_URL: string =
  (import.meta.env?.CONVA_SUPABASE_URL as string | undefined) || DEFAULT_SUPABASE_URL;
const ANON_KEY: string =
  (import.meta.env?.CONVA_SUPABASE_ANON_KEY as string | undefined) || DEFAULT_ANON_KEY;

/**
 * The website's login page (Layer 2 — conva_web). The web app is a SECTION of
 * the website (same origin, e.g. getconva.com/app), and login is the website's
 * job — so in production login is simply `/login.html` on this same origin.
 * Local dev runs the site on a different port, so allow an override there.
 */
function loginUrl(): string {
  if (typeof window === "undefined") return "/login.html";
  const { hostname } = window.location;
  const local = hostname === "localhost" || hostname === "127.0.0.1";
  if (local) {
    return (import.meta.env?.CONVA_WEB_LOGIN_URL as string | undefined) || "/login.html";
  }
  return "/login.html";
}

interface WebSession {
  access_token: string;
  refresh_token: string | null;
  expires_at: number | null; // unix seconds
  email: string | null;
  user_id: string | null;
}

// ------------------------------------------------------------- session store

export function loadSession(): WebSession | null {
  try {
    const raw = localStorage.getItem(SESSION_KEY);
    return raw ? (JSON.parse(raw) as WebSession) : null;
  } catch {
    return null;
  }
}

/** Persist a GoTrue token response the same way auth.js does. */
function saveSession(t: {
  access_token: string;
  refresh_token?: string;
  expires_at?: number;
  expires_in?: number;
  user?: { email?: string; id?: string };
}): WebSession {
  const expires_at =
    t.expires_at ?? (t.expires_in ? Math.floor(Date.now() / 1000) + t.expires_in : null);
  const session: WebSession = {
    access_token: t.access_token,
    refresh_token: t.refresh_token ?? null,
    expires_at,
    email: t.user?.email ?? null,
    user_id: t.user?.id ?? null,
  };
  localStorage.setItem(SESSION_KEY, JSON.stringify(session));
  notify();
  return session;
}

function clearSession(): void {
  localStorage.removeItem(SESSION_KEY);
  notify();
}

export function toStatus(s: WebSession | null): AuthStatus {
  const expired = s?.expires_at != null && s.expires_at <= Math.floor(Date.now() / 1000);
  const signed_in = !!s?.access_token && !expired;
  return {
    signed_in,
    email: signed_in ? (s?.email ?? null) : null,
    user_id: signed_in ? (s?.user_id ?? null) : null,
    expires_at_unix: signed_in ? (s?.expires_at ?? null) : null,
    configured: !!ANON_KEY,
  };
}

// ------------------------------------------------- change notification (authChanged)

type Listener = (status: AuthStatus) => void;
const listeners = new Set<Listener>();

function notify(): void {
  const status = toStatus(loadSession());
  for (const l of listeners) l(status);
}

/** Subscribe to session changes (sign-in/out here, or in another tab — the
 *  `storage` event covers the login page completing in a separate tab). */
export function onAuthChanged(l: Listener): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

if (typeof window !== "undefined") {
  window.addEventListener("storage", (e) => {
    if (e.key === SESSION_KEY) notify();
  });

  // SSO hand-off intake: the shared login page returns here with the session
  // in the URL fragment (#conva_session=base64url — never sent to a server).
  // Store it as our own session and scrub the URL. See conva_web/login.html.
  try {
    const m = /[#&]conva_session=([A-Za-z0-9_-]+)/.exec(window.location.hash);
    if (m?.[1]) {
      const json = atob(m[1].replace(/-/g, "+").replace(/_/g, "/"));
      const s = JSON.parse(json) as Partial<WebSession>;
      if (typeof s.access_token === "string" && s.access_token.length > 0) {
        localStorage.setItem(
          SESSION_KEY,
          JSON.stringify({
            access_token: s.access_token,
            refresh_token: s.refresh_token ?? null,
            expires_at: s.expires_at ?? null,
            email: s.email ?? null,
            user_id: s.user_id ?? null,
          } satisfies WebSession),
        );
      }
      history.replaceState(null, "", window.location.pathname + window.location.search);
      // Listeners registered after module load still see the fresh session via
      // status(); anyone already subscribed gets the change notification.
      setTimeout(notify, 0);
    }
  } catch {
    /* malformed fragment — ignore */
  }
}

// ----------------------------------------------------------------- GoTrue REST

class AuthError extends Error {}

async function post<T>(path: string, body: unknown, token?: string): Promise<T> {
  const headers: Record<string, string> = {
    apikey: ANON_KEY,
    "Content-Type": "application/json",
  };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(`${SUPABASE_URL}/auth/v1/${path}`, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok) {
    const msg =
      (data.error_description as string) ||
      (data.msg as string) ||
      (data.message as string) ||
      `Auth request failed (${res.status})`;
    throw new AuthError(msg);
  }
  return data as T;
}

// -------------------------------------------------------------------- actions

export async function signinPassword(email: string, password: string): Promise<AuthStatus> {
  const data = await post<Parameters<typeof saveSession>[0]>(
    "token?grant_type=password",
    { email: email.trim(), password },
  );
  return toStatus(saveSession(data));
}

export async function signupPassword(email: string, password: string): Promise<AuthStatus> {
  const data = await post<Parameters<typeof saveSession>[0] & { access_token?: string }>(
    "signup",
    { email: email.trim(), password },
  );
  // A session comes back only when email confirmation is off; otherwise the
  // caller sees a signed-out status and the UI says "check your email".
  if (data.access_token) return toStatus(saveSession(data as Parameters<typeof saveSession>[0]));
  return toStatus(null);
}

export async function signout(): Promise<void> {
  const s = loadSession();
  if (s?.access_token) {
    // Best-effort server-side revoke; local clear is what matters.
    await post("logout", {}, s.access_token).catch(() => {});
  }
  clearSession();
}

export function status(): AuthStatus {
  return toStatus(loadSession());
}

// ------------------------------------------------------------- token claims

/** Decoded JWT payload of the current access token (web only), or null. */
export function jwtClaims(): Record<string, unknown> | null {
  const s = loadSession();
  const part = s?.access_token?.split(".")[1];
  if (!part) return null;
  try {
    return JSON.parse(atob(part.replace(/-/g, "+").replace(/_/g, "/"))) as Record<
      string,
      unknown
    >;
  } catch {
    return null;
  }
}

/** The identity provider of the current session ("email", "google", …). */
export function provider(): string | null {
  const meta = jwtClaims()?.app_metadata as { provider?: string } | undefined;
  return meta?.provider ?? null;
}

/**
 * The beta-allowlist entitlement (roadmap 1.2 / owner decision D1): true when
 * the user's `app_metadata.beta_access` is set (flipped per-user in the
 * Supabase dashboard until the application form lands). Swaps to a
 * subscription predicate when billing arrives — same gate, new predicate.
 * null = signed out / undecodable.
 */
export function betaAccess(): boolean | null {
  const claims = jwtClaims();
  if (!claims) return null;
  const meta = claims.app_metadata as { beta_access?: unknown } | undefined;
  return meta?.beta_access === true;
}

/** OAuth / full login: hand off to the shared login page, returning here after
 *  (session comes back in the URL fragment — see the intake above). */
export function loginRedirect(provider?: string): void {
  const back = encodeURIComponent(window.location.href);
  const prov = provider ? `&provider=${encodeURIComponent(provider)}` : "";
  window.location.href = `${loginUrl()}?return=${back}${prov}`;
}
