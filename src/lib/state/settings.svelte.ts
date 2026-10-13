import { storedPresets } from "$lib/assistant";
import type { Preset } from "$lib/assistant";
import { readSettings, writeSetting } from "$lib/ipc";
import { messageOf } from "$lib/utils";

export type Settings = {
  workspace: string | null;
  project: string | null;
  recentProjects: string[];
  lastPage: string | null;
  fontSize: number;
  wordWrap: boolean;
  autosave: boolean;
  zoom: number | null;
  theme: "light" | "dark";
  language: string | null;
  uiTheme: string | null;
  hiddenTemplates: string[];
  hiddenLooks: string[];
  assistantProvider: string | null;
  assistantPreset: string | null;
  assistantModels: Record<string, string>;
  assistantPresets: Record<string, Preset[]>;
};

type Persist = (key: string, value: unknown) => Promise<void>;

export const fontSizes = { min: 8, max: 32 };

export const defaults: Settings = {
  workspace: null,
  project: null,
  recentProjects: [],
  lastPage: null,
  fontSize: 14,
  wordWrap: true,
  autosave: true,
  zoom: null,
  theme: "light",
  language: null,
  uiTheme: null,
  hiddenTemplates: [],
  hiddenLooks: [],
  assistantProvider: null,
  assistantPreset: null,
  assistantModels: {},
  assistantPresets: {},
};

export function merged(stored: Record<string, unknown>, osPrefersDark: boolean): Settings {
  const text = (value: unknown) => (typeof value === "string" ? value : null);
  const flag = (value: unknown, fallback: boolean) => (typeof value === "boolean" ? value : fallback);
  const { fontSize, zoom, theme } = stored;
  const ids = (value: unknown) => (Array.isArray(value) ? value.filter((id) => typeof id === "string") : []);
  const names = (value: unknown) =>
    Object.fromEntries(Object.entries(typeof value === "object" && value !== null ? value : {}).filter((entry): entry is [string, string] => typeof entry[1] === "string"));
  return {
    workspace: text(stored.workspace),
    project: text(stored.project),
    recentProjects: ids(stored.recentProjects),
    lastPage: text(stored.lastPage),
    fontSize:
      typeof fontSize === "number" && fontSize >= fontSizes.min && fontSize <= fontSizes.max
        ? fontSize
        : defaults.fontSize,
    wordWrap: flag(stored.wordWrap, defaults.wordWrap),
    autosave: flag(stored.autosave, defaults.autosave),
    zoom: typeof zoom === "number" && zoom > 0 ? zoom : null,
    theme: theme === "light" || theme === "dark" ? theme : osPrefersDark ? "dark" : "light",
    language: text(stored.language),
    uiTheme: text(stored.uiTheme),
    hiddenTemplates: ids(stored.hiddenTemplates),
    hiddenLooks: ids(stored.hiddenLooks),
    assistantProvider: text(stored.assistantProvider),
    assistantPreset: text(stored.assistantPreset),
    assistantModels: names(stored.assistantModels),
    assistantPresets: storedPresets(stored.assistantPresets),
  };
}

export class SettingsState {
  values = $state<Settings>({ ...defaults });
  loaded = $state(false);
  failure = $state<string | null>(null);

  constructor(private persist: Persist) {}

  async load(read: () => Promise<Record<string, unknown>>, osPrefersDark: boolean) {
    this.values = merged(await read(), osPrefersDark);
    this.loaded = true;
  }

  set<Key extends keyof Settings>(key: Key, value: Settings[Key]) {
    if (this.values[key] === value) return;
    this.values[key] = value;
    this.persist(key, value).then(
      () => (this.failure = null),
      (error: unknown) => (this.failure = messageOf(error)),
    );
  }
}

export const settings = new SettingsState(writeSetting);

export const loadSettings = () =>
  settings.load(readSettings, matchMedia("(prefers-color-scheme: dark)").matches);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  test("saved settings win, and anything missing or wrong falls back to the default", () => {
    const stored = { workspace: "C:\\notes", fontSize: 18, wordWrap: false, zoom: 1.5, theme: "light", uiTheme: "acme", hiddenTemplates: ["acme"], hiddenLooks: ["acme"] };
    expect(merged(stored, true)).toEqual({ ...defaults, ...stored });
    expect(merged({ fontSize: "huge", wordWrap: 1, zoom: -1, theme: "pink", workspace: 7, uiTheme: 3, hiddenTemplates: "all", hiddenLooks: [7] }, false)).toEqual(defaults);
    expect(merged({ fontSize: 400 }, false).fontSize).toBe(defaults.fontSize);
    expect(merged({ assistantModels: { custom: "llama", claude: 7 }, assistantProvider: "custom" }, false)).toMatchObject({ assistantModels: { custom: "llama" }, assistantProvider: "custom" });
  });

  test("with no saved theme the OS decides", () => {
    expect(merged({}, true).theme).toBe("dark");
    expect(merged({}, false).theme).toBe("light");
    expect(merged({ theme: "light" }, true).theme).toBe("light");
  });

  test("a changed setting is stored once and an unchanged one is not", () => {
    const persist = vi.fn<Persist>(() => Promise.resolve());
    const state = new SettingsState(persist);
    state.set("fontSize", 20);
    state.set("fontSize", 20);
    expect(state.values.fontSize).toBe(20);
    expect(persist.mock.calls).toEqual([["fontSize", 20]]);
  });
}
