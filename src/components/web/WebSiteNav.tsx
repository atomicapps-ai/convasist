import mark from "@/assets/brand/conva-mark-cutout-white.svg";
import * as webAuth from "@/lib/backend/webAuth";

/*
 * The TOP band of the web experience: the core WEBSITE navigation, rendered by
 * the app so it's always present above the app's own icon nav (owner spec:
 * "top level = core website links, below that = app navigation icons").
 *
 * These are links to the marketing/account site (conva_web) — login and the
 * account/profile pages live THERE, the app only links out. `target="_top"`
 * breaks out of the embedding iframe so the whole window navigates to the site.
 * The site origin is passed in by the host page (?site=…); standalone falls back
 * to the current origin.
 */
const SITE_ORIGIN =
  (typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).get("site")) ||
  (typeof window !== "undefined" ? window.location.origin : "");

const site = (path: string) => `${SITE_ORIGIN}${path}`;

const LINKS = [
  { label: "Product", href: "/#features" },
  { label: "Pricing", href: "/#pricing" },
  { label: "Beta", href: "/#join" },
];

export function WebSiteNav() {
  const email = webAuth.status().email;
  const initial = (email?.trim()?.[0] ?? "?").toUpperCase();

  return (
    <header className="flex h-[52px] shrink-0 items-center gap-5 border-b border-border bg-panel-raised px-4">
      <a
        href={site("/")}
        target="_top"
        aria-label="conva home"
        className="flex items-center gap-2 text-fg no-underline"
      >
        <img src={mark} alt="" className="h-[22px] w-[22px]" draggable={false} />
        <span className="text-[15px] font-extrabold tracking-tight">conva</span>
      </a>

      <nav aria-label="Site" className="flex items-center gap-4 text-sm">
        {LINKS.map((l) => (
          <a
            key={l.href}
            href={site(l.href)}
            target="_top"
            className="text-fg-muted no-underline transition hover:text-fg"
          >
            {l.label}
          </a>
        ))}
      </nav>

      <span className="ml-auto" />

      {/* Account access — a LINK to the website account page (login/profile live
          there, not in the app). */}
      <a
        href={site("/account.html")}
        target="_top"
        title={email ?? "Your account"}
        aria-label="Your account"
        className="brand-gradient grid h-8 w-8 place-items-center rounded-full text-sm font-extrabold text-bg no-underline transition hover:brightness-110"
      >
        {initial}
      </a>
    </header>
  );
}
