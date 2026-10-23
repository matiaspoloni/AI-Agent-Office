// How the user dressed each project's agents. Keyed by team (the project an
// agent works for, see office/teams.ts), so every session of a project keeps
// its look across restarts. Purely cosmetic, so it lives in the webview's
// local storage rather than the core's database; unset fields fall back to
// the look derived from the agent key.

import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { Accessory, HairStyle } from "../office/sprites";

export interface Appearance {
  style?: HairStyle;
  hair?: string;
  skin?: string;
  /** Replaces the provider color on the shirt (the badge still shows the provider). */
  shirt?: string;
  accessory?: Accessory;
  /** Shown on the name tag of the project's main agents. */
  nickname?: string;
}

interface AppearanceState {
  byTeam: Record<string, Appearance>;
  update(team: string, patch: Appearance): void;
  reset(team: string): void;
}

export const useAppearanceStore = create<AppearanceState>()(
  persist(
    (set) => ({
      byTeam: {},
      update(team, patch) {
        set((state) => {
          const next: Appearance = { ...state.byTeam[team], ...patch };
          for (const k of Object.keys(next) as (keyof Appearance)[]) if (next[k] === undefined || next[k] === "") delete next[k];
          return { byTeam: { ...state.byTeam, [team]: next } };
        });
      },
      reset(team) {
        set((state) => {
          const byTeam = { ...state.byTeam };
          delete byTeam[team];
          return { byTeam };
        });
      },
    }),
    { name: "agent-office.appearance", version: 1, storage: createJSONStorage(() => localStorage) },
  ),
);
