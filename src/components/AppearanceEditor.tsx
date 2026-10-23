import { useMemo } from "react";
import type { AgentState } from "../bindings/AgentState";
import type { SessionState } from "../bindings/SessionState";
import { teamResolver } from "../office/teams";
import {
  ACCESSORIES,
  type Accessory,
  characterCanvas,
  characterLook,
  HAIR,
  HAIR_STYLES,
  type HairStyle,
  SHIRTS,
  SKIN,
} from "../office/sprites";
import { useAppearanceStore } from "../state/appearance";
import { useOfficeStore } from "../state/store";

const STYLE_LABEL: Record<HairStyle, string> = { short: "Short", long: "Long", bun: "Bun", cap: "Cap" };
const ACCESSORY_LABEL: Record<Accessory, string> = {
  none: "None",
  glasses: "Glasses",
  headphones: "Headphones",
  beard: "Beard",
  bow: "Bow",
};
const HAIR_NAMES = ["Black", "Dark brown", "Brown", "Blond", "Platinum", "Red", "Charcoal", "Grey"];
const SHIRT_NAMES = ["Coral", "Blue", "Green", "Amber", "Violet", "Teal", "Pink", "White"];

function Swatches({
  legend,
  colors,
  names,
  value,
  onPick,
}: {
  legend: string;
  colors: string[];
  names: string[];
  value: string;
  onPick: (color: string) => void;
}) {
  return (
    <fieldset className="appearance-group">
      <legend>{legend}</legend>
      <div className="swatches">
        {colors.map((color, i) => (
          <button
            key={`${color}-${i}`}
            type="button"
            className="swatch"
            style={{ background: color }}
            aria-pressed={color === value}
            aria-label={names[i]}
            title={names[i]}
            onClick={() => onPick(color)}
          />
        ))}
      </div>
    </fieldset>
  );
}

function Choices<T extends string>({
  legend,
  options,
  labels,
  value,
  onPick,
}: {
  legend: string;
  options: T[];
  labels: Record<T, string>;
  value: T;
  onPick: (option: T) => void;
}) {
  return (
    <fieldset className="appearance-group">
      <legend>{legend}</legend>
      <div className="segmented">
        {options.map((option) => (
          <button key={option} type="button" className="btn small" aria-pressed={option === value} onClick={() => onPick(option)}>
            {labels[option]}
          </button>
        ))}
      </div>
    </fieldset>
  );
}

/**
 * How this agent's project looks in the office: every agent of the project
 * shares it (subagents in a lighter shirt), and it is kept across sessions.
 */
export function AppearanceEditor({ agent, session }: { agent: AgentState; session: SessionState }) {
  const projects = useOfficeStore((s) => s.projects);
  const descriptor = useOfficeStore((s) => s.providers[agent.provider]?.descriptor);
  const team = useMemo(() => teamResolver({ [session.key]: session }, projects)(agent), [agent, session, projects]);
  const custom = useAppearanceStore((s) => s.byTeam[team.key]);
  const update = useAppearanceStore((s) => s.update);
  const reset = useAppearanceStore((s) => s.reset);

  const accent = descriptor?.accentColor || "#9aa3b5";
  const look = characterLook(agent.key, accent, !agent.isMain, custom);
  const preview = useMemo(() => characterCanvas(look, "stand")?.toDataURL() ?? "", [look.id]);
  const set = (patch: Parameters<typeof update>[1]) => update(team.key, patch);
  const shirt = custom?.shirt ?? accent;
  // The provider's own color comes first; skip it in the palette if it is there too.
  const others = SHIRTS.map((color, i) => ({ color, name: SHIRT_NAMES[i] })).filter((s) => s.color !== accent);

  return (
    <section className="panel-section">
      <h4>Appearance</h4>
      <div className="appearance">
        <div className="appearance-preview">{preview && <img src={preview} alt="" width={52} height={68} />}</div>
        <div>
          <label>
            Name tag
            <input
              value={custom?.nickname ?? ""}
              placeholder={agent.isMain ? agent.name : "Shown on the project's main agents"}
              maxLength={24}
              onChange={(e) => set({ nickname: e.target.value })}
            />
          </label>
          <Choices legend="Hair style" options={HAIR_STYLES} labels={STYLE_LABEL} value={look.style} onPick={(style) => set({ style })} />
        </div>
      </div>
      <Swatches legend="Hair" colors={HAIR} names={HAIR_NAMES} value={look.palette.h} onPick={(hair) => set({ hair })} />
      <Swatches
        legend="Skin"
        colors={SKIN}
        names={SKIN.map((_, i) => `Skin tone ${i + 1}`)}
        value={look.palette.s}
        onPick={(skin) => set({ skin })}
      />
      <Swatches
        legend="Shirt"
        colors={[accent, ...others.map((s) => s.color)]}
        names={[`${descriptor?.displayName ?? "Provider"} color (default)`, ...others.map((s) => s.name)]}
        value={shirt}
        onPick={(color) => set({ shirt: color === accent ? undefined : color })}
      />
      <Choices legend="Accessory" options={ACCESSORIES} labels={ACCESSORY_LABEL} value={look.accessory} onPick={(accessory) => set({ accessory })} />
      <div className="button-row">
        <span className="muted small grow">
          Every agent of {team.name} looks like this; subagents wear a lighter shirt. The badge still shows the provider.
        </span>
        <button className="btn small" disabled={!custom} onClick={() => reset(team.key)}>
          Reset
        </button>
      </div>
    </section>
  );
}
