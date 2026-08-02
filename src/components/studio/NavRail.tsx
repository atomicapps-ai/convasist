import { useEffect, useState } from "react";

import mark from "@/assets/brand/conva-mark-cutout-white.svg";
import { Icon, type IconName } from "@/components/ui/Icon";
import { useBackend } from "@/lib/backend";
import { isTauri, type AuthStatus } from "@/lib/ipc";
import { PLATFORM } from "@/lib/platform";
import { useNavStore, type View } from "@/state/nav";

/**
 * The Studio's left instrument rail (52px). View selectors with the conva mark
 * up top and the ⌘K palette trigger at the foot. The active view gets a 3px
 * violet indicator on its leading edge and a violet-tinted chip (brand v2).
 */

/**
 * The rail is data-driven so the two platforms extend ONE base list instead of
 * forking the component. `only` omitted → shown on both (base); set it to gate a
 * row to a platform. Web-only rows (an "Upgrade" CTA, say) just get
 * `only: "web"`; desktop-only rows `only: "desktop"`. This is the "web adds on
 * top of the base nav" seam — composition, not a NavRail subclass.
 */
type NavItem = { view: View; icon: IconName; label: string; only?: "web" | "desktop" };

const NAV_ITEMS: NavItem[] = [
  { view: "dashboard", icon: "system", label: "Home" },
  { view: "live", icon: "live", label: "Live" },
  { view: "conversations", icon: "conversations", label: "Conversations" },
  { view: "simcon", icon: "simicon", label: "Sim Con" },
  { view: "sessions", icon: "sessions", label: "Sessions" },
  { view: "library", icon: "library", label: "Library" },
  { view: "features", icon: "book", label: "What conva does" },
  { view: "whatsnew", icon: "lightbulb", label: "What's Coming" },
  { view: "settings", icon: "settings", label: "Settings" },
  // e.g. a future web-only entry:  { view: "upgrade", icon: "lightbulb", label: "Upgrade", only: "web" },
];

/** The base list resolved for THIS platform (base rows + this platform's rows). */
const RAIL_ITEMS = NAV_ITEMS.filter((i) => !i.only || i.only === PLATFORM);

function RailButton({
  active,
  label,
  onClick,
  children,
}: {
  active?: boolean;
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      className={[
        "group relative flex h-[30px] w-[30px] items-center justify-center rounded-sm border transition",
        active
          ? "border-outbound/34 bg-outbound/[0.14] text-[var(--voice-you-text)]"
          : "border-transparent text-fg-faint hover:bg-panel-raised/60 hover:text-fg",
      ].join(" ")}
    >
      {/* 3px violet indicator on the leading edge of the active view. */}
      <span
        className={[
          "absolute -left-2 h-4 w-[3px] rounded-full bg-outbound transition-opacity",
          active ? "opacity-100" : "opacity-0",
        ].join(" ")}
        aria-hidden
      />
      {children}
    </button>
  );
}

export function NavRail() {
  const backend = useBackend();
  const view = useNavStore((s) => s.view);
  const setView = useNavStore((s) => s.setView);
  const openPalette = useNavStore((s) => s.openPalette);
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  // Reflect sign-in state on the account button. Refreshes when the view
  // changes so signing in via Settings updates the rail without a reload.
  useEffect(() => {
    setMenuOpen(false);
    if (!isTauri()) return;
    void backend.auth
      .status()
      .then(setAuth)
      .catch(() => setAuth(null));
  }, [view, backend]);

  return (
    <nav
      aria-label="Primary"
      className="glass z-10 flex w-[44px] shrink-0 flex-col items-center gap-1 rounded-r-lg border-l-0 py-2"
    >
      {/* Brand mark. */}
      <img
        src={mark}
        alt="conva"
        title="conva"
        draggable={false}
        className="mb-2 h-[22px] w-[22px]"
      />

      {RAIL_ITEMS.map((item) => (
        <RailButton
          key={item.view}
          active={view === item.view}
          label={item.label}
          onClick={() => setView(item.view)}
        >
          <Icon name={item.icon} size={20} />
        </RailButton>
      ))}

      <div className="mt-auto flex flex-col items-center gap-1.5 pt-2">
        <RailButton label="Command palette (⌘K)" onClick={openPalette}>
          <Icon name="search" size={19} />
        </RailButton>

        {/* Floating HUD panel — always-on-top, non-activating overlay. */}
        <RailButton
          label="Floating HUD"
          onClick={() => {
            if (isTauri()) void backend.hud.toggle().catch(() => {});
          }}
        >
          <Icon name="expand" size={18} />
        </RailButton>

        {/* Account. Signed in: hover shows the email; click opens a small menu
            (Profile / Settings). Signed out: click jumps to Settings to sign in. */}
        <div className="relative">
          <RailButton
            label={
              auth?.signed_in
                ? `${auth.email ?? "Signed in"} — account menu`
                : "Account — sign in"
            }
            active={menuOpen}
            onClick={() => {
              if (auth?.signed_in) setMenuOpen((o) => !o);
              else setView("settings");
            }}
          >
            <span className="relative flex items-center justify-center">
              <Icon name="account" size={20} />
              {auth?.signed_in && (
                <span
                  className="absolute -right-1 -top-1 h-2 w-2 rounded-full bg-ok"
                  aria-hidden
                />
              )}
            </span>
          </RailButton>

          {menuOpen && auth?.signed_in && (
            <>
              <button
                type="button"
                aria-label="Close account menu"
                onClick={() => setMenuOpen(false)}
                className="fixed inset-0 z-40 cursor-default"
              />
              <div
                role="menu"
                className="glass-raised absolute bottom-0 left-[calc(100%+8px)] z-50 w-[188px] rounded-lg border border-border-strong p-1 shadow-[var(--shadow-lg)]"
              >
                <p className="truncate px-2.5 py-1.5 text-[11px] text-fg-faint">
                  {auth.email ?? "Signed in"}
                </p>
                <span className="mx-1 mb-1 block h-px bg-border" />
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    setView("settings");
                  }}
                  className="flex w-full items-center gap-2.5 rounded-[5px] px-2.5 py-1.5 text-left text-xs text-fg hover:bg-panel-raised/70"
                >
                  <Icon name="account" size={15} className="text-fg-muted" />
                  Profile
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    setView("settings");
                  }}
                  className="flex w-full items-center gap-2.5 rounded-[5px] px-2.5 py-1.5 text-left text-xs text-fg hover:bg-panel-raised/70"
                >
                  <Icon name="settings" size={15} className="text-fg-muted" />
                  Settings
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </nav>
  );
}
