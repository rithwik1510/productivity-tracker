import { focusManager } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";
import { commands } from "../api/bindings";
import { useSettings } from "../api/queries";

/**
 * The one place that decides data-surface: glass iff Appearance is Glass, Windows' Transparency
 * effects are on (re-checked on every focus gain) and the window is focused, because Windows
 * drops the Acrylic backdrop otherwise. Until settings load the root keeps index.html's solid.
 * Focus also drives TanStack's focusManager, so queries refetch on focus the Tauri way rather
 * than the browser's.
 */
export function useSurface() {
  const appearance = useSettings().data?.appearance;

  useEffect(() => {
    if (appearance === undefined) return;
    const root = document.documentElement;
    let latest = 0;
    const apply = async (focused: boolean) => {
      const call = ++latest;
      focusManager.setFocused(focused);
      const glass =
        appearance === "glass" && focused && (await commands.glassAvailable().catch(() => false));
      if (call !== latest) return; // a newer focus change already won
      const surface = glass ? "glass" : "solid";
      if (root.dataset.surface === surface) return;
      // Swap tokens with transitions off, so 150ms color fades don't smear the switch.
      root.dataset.switching = "";
      root.dataset.surface = surface;
      requestAnimationFrame(() => requestAnimationFrame(() => delete root.dataset.switching));
    };

    // Not `isFocused()`: on Windows it reads the top-level window, which loses keyboard focus to
    // the WebView2 child, so it says false while the window is in front. The focus events come
    // from the webview and are right, and so is the document's own focus. Read it again once the
    // listener is live, in case focus changed while it was registering.
    void apply(document.hasFocus());
    const unlisten = getCurrentWindow().onFocusChanged(({ payload }) => void apply(payload));
    void unlisten.then(() => apply(document.hasFocus()));
    return () => {
      latest += 1; // drop a check still in flight for the old Appearance
      void unlisten.then((stop) => stop());
    };
  }, [appearance]);
}
