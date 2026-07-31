import { NAV_ITEMS } from "@/components/studio/navItems";
import { Icon } from "@/components/ui/Icon";
import { useNavStore } from "@/state/nav";

/*
 * The embedded app's own navigation — a horizontal ICON bar (the web echo of
 * the desktop left rail). It sits UNDER the website header (which owns brand,
 * core site links, and account/login), so this bar carries ZERO auth/account:
 * the app is purely functionality + features. View icons only.
 */
const ITEMS = NAV_ITEMS.filter((i) => !i.only || i.only === "web");

export function WebTopNav() {
  const view = useNavStore((s) => s.view);
  const setView = useNavStore((s) => s.setView);

  return (
    <nav
      aria-label="App"
      className="flex h-12 shrink-0 items-center gap-1 border-b border-border bg-panel px-3"
    >
      {ITEMS.map((item) => {
        const active = view === item.view;
        return (
          <button
            key={item.view}
            type="button"
            onClick={() => setView(item.view)}
            title={item.label}
            aria-label={item.label}
            aria-current={active ? "page" : undefined}
            className={[
              "grid h-9 w-9 place-items-center rounded-lg border transition",
              active
                ? "border-outbound/34 bg-outbound/[0.14] text-[var(--voice-you-text)]"
                : "border-transparent text-fg-faint hover:bg-panel-raised/60 hover:text-fg",
            ].join(" ")}
          >
            <Icon name={item.icon} size={20} />
          </button>
        );
      })}
    </nav>
  );
}
