// The office theme (modern or cyberpunk). It paints the office canvas and,
// through `data-theme` on <html>, recolors the interface around it. A
// per-viewer preference, kept in the webview's local storage.

import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { OfficeTheme } from "../office/renderer";

interface ThemeState {
  theme: OfficeTheme;
  setTheme(theme: OfficeTheme): void;
}

export const useThemeStore = create<ThemeState>()(
  persist(
    (set) => ({
      theme: "modern",
      setTheme(theme) {
        set({ theme });
      },
    }),
    { name: "agent-office.theme", version: 1, storage: createJSONStorage(() => localStorage) },
  ),
);

function apply(theme: OfficeTheme) {
  if (typeof document !== "undefined") document.documentElement.dataset.theme = theme;
}
apply(useThemeStore.getState().theme);
useThemeStore.subscribe((s) => apply(s.theme));
