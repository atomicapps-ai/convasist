import type { IconName } from "@/components/ui/Icon";
import type { View } from "@/state/nav";

/**
 * The one source of truth for the app's primary navigation, consumed by BOTH
 * the desktop left rail (NavRail) and the web top nav (WebTopNav). `only`
 * omitted → shown on both platforms; set it to gate a row to one.
 */
export type NavItem = {
  view: View;
  icon: IconName;
  label: string;
  only?: "web" | "desktop";
};

export const NAV_ITEMS: NavItem[] = [
  { view: "dashboard", icon: "system", label: "Home" },
  { view: "live", icon: "live", label: "Live" },
  { view: "conversations", icon: "conversations", label: "Conversations" },
  { view: "sessions", icon: "sessions", label: "Sessions" },
  { view: "library", icon: "library", label: "Library" },
  { view: "features", icon: "book", label: "What conva does" },
  { view: "whatsnew", icon: "lightbulb", label: "What's Coming" },
  { view: "releases", icon: "sparkle", label: "What's New" },
  { view: "settings", icon: "settings", label: "Settings" },
];
