/**
 * Sidecar mode (design §4.3 U9): a narrow always-on-top strip that sits
 * beside a full-screen call window. Window sizing is remembered and
 * restored when toggled off.
 */

import { isTauri } from "@/lib/ipc";

const SIDECAR_WIDTH = 380;

let savedSize: { width: number; height: number } | null = null;

export async function applySidecar(on: boolean): Promise<void> {
  if (!isTauri()) return;
  const { getCurrentWindow, LogicalSize } = await import(
    "@tauri-apps/api/window"
  );
  const win = getCurrentWindow();

  if (on) {
    const inner = await win.innerSize();
    const factor = await win.scaleFactor();
    savedSize = {
      width: inner.width / factor,
      height: inner.height / factor,
    };
    await win.setAlwaysOnTop(true);
    await win.setSize(new LogicalSize(SIDECAR_WIDTH, savedSize.height));
  } else {
    await win.setAlwaysOnTop(false);
    if (savedSize) {
      await win.setSize(new LogicalSize(savedSize.width, savedSize.height));
      savedSize = null;
    }
  }
}
