import { describe, expect, it } from "vitest";
import { useAppearanceStore } from "./appearance";

describe("appearance store", () => {
  it("merges changes per team, drops cleared fields and resets", () => {
    const { update, reset } = useAppearanceStore.getState();
    update("project:a", { hair: "#b83c2e", nickname: "Ada" });
    update("project:a", { style: "bun", nickname: "" });
    update("project:b", { shirt: "#4c8df5" });
    expect(useAppearanceStore.getState().byTeam["project:a"]).toEqual({ hair: "#b83c2e", style: "bun" });
    expect(useAppearanceStore.getState().byTeam["project:b"]).toEqual({ shirt: "#4c8df5" });
    reset("project:a");
    expect(useAppearanceStore.getState().byTeam["project:a"]).toBeUndefined();
    expect(JSON.parse(localStorage.getItem("agent-office.appearance") ?? "{}").state.byTeam).toEqual({ "project:b": { shirt: "#4c8df5" } });
  });
});
