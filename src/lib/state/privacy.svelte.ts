import * as ipc from "$lib/ipc";
import type { KnowledgeStatus, ModelId, PrivacyOptions, PrivacyPolicy, Progress } from "$lib/ipc";
import { tokens } from "$lib/markdown";
import { messageOf } from "$lib/utils";
import { SettingsState, settings } from "./settings.svelte";
import type { Settings } from "./settings.svelte";

type Backend = Pick<
  typeof ipc,
  | "knowledgeStatus"
  | "knowledgeCreate"
  | "knowledgePick"
  | "knowledgeImport"
  | "knowledgeUnlock"
  | "knowledgeActivate"
  | "knowledgeExport"
  | "knowledgePassword"
  | "knowledgeRemove"
  | "knowledgeAdd"
  | "knowledgeAttach"
  | "knowledgeForget"
  | "knowledgeRebuild"
  | "glossaryImport"
  | "modelsDownload"
  | "modelsCancel"
  | "privacyView"
  | "privacyOverride"
  | "privacyPolicy"
>;

// Glossary categories, then the pattern kinds in detection.json; a test keeps them in step.
export const kinds = ["CUSTOMER", "PERSON", "ORGANISATION", "SUPPLIER", "PROJECT", "PRODUCT", "LOCATION", "AMOUNT", "EMAIL", "PHONE", "URL", "IBAN", "CARD", "IP"];

// Fixed hues, one per kind, spread around the colour wheel so kinds tell apart at a glance in
// every theme. The 30% fill keeps the text readable on light and dark backgrounds.
const kindColours: Record<string, string> = {
  PERSON: "bg-blue-500/30",
  ORGANISATION: "bg-violet-500/30",
  CUSTOMER: "bg-green-500/30",
  SUPPLIER: "bg-yellow-400/40",
  PROJECT: "bg-fuchsia-500/30",
  PRODUCT: "bg-cyan-400/40",
  LOCATION: "bg-lime-400/40",
  AMOUNT: "bg-red-500/30",
  EMAIL: "bg-orange-500/30",
  PHONE: "bg-teal-600/30",
  URL: "bg-indigo-700/30",
  IBAN: "bg-pink-400/40",
  CARD: "bg-amber-700/30",
  IP: "bg-stone-500/30",
};

export const kindColour = (kind: string | null) => (kind !== null ? kindColours[kind] : undefined) ?? "bg-muted-foreground/20";


type Ruled = "hideNames" | "findNames" | "hideAmounts";

export function optionsOf(values: Settings, status: KnowledgeStatus | null, policy: PrivacyPolicy | null = null): PrivacyOptions {
  const usable = status?.active != null;
  const required = (rule: Ruled) => policy?.[rule] === "required";
  return {
    veil: values.privacyVeil || required("hideNames"),
    amounts: values.privacyAmounts || required("hideAmounts"),
    names: values.privacyNames || required("findNames"),
    knowledge: values.knowledgeOn && usable,
    autoAdd: values.knowledgeAutoAdd,
  };
}

export type Blocked = "noFile" | "locked" | "noFinder";

export function blockedBy(options: PrivacyOptions, status: KnowledgeStatus | null): Blocked | null {
  if (!options.veil) return null;
  if (status?.active == null) return "noFile";
  if (!status.open) return "locked";
  return options.names && !status.names ? "noFinder" : null;
}

export const linesToHide = (picked: string) => [...new Set(picked.split("\n").map((line) => line.trim()))].filter((line) => line !== "" && line.length <= 200).slice(0, 50);

export const hiddenCount = (counted: Record<string, number>) => Object.values(counted).reduce((sum, count) => sum + count, 0);

export class Privacy {
  status = $state<KnowledgeStatus | null>(null);
  policy = $state<PrivacyPolicy | null>(null);
  failure = $state<string | null>(null);
  busy = $state(false);
  progress = $state<Partial<Record<ModelId, Progress>>>({});
  viewing = $state(false);
  changes = $state(0);
  unlockDismissed = $state(false);

  constructor(
    private backend: Backend,
    private settings: SettingsState,
  ) {}

  get options() {
    return optionsOf(this.settings.values, this.status, this.policy);
  }

  get blocked() {
    return blockedBy(this.options, this.status);
  }

  required(rule: Ruled) {
    return this.policy?.[rule] === "required";
  }

  get usable() {
    return this.status?.active != null;
  }

  get open() {
    return this.status?.open ?? false;
  }

  get locked() {
    return this.usable && !this.open;
  }

  get activeName() {
    return this.status?.files.find(({ id }) => id === this.status?.active)?.name ?? null;
  }

  get hiding() {
    return this.options.veil;
  }

  private async run<T>(task: () => Promise<T>) {
    this.busy = true;
    this.failure = null;
    try {
      const done = await task();
      if (typeof done === "object" && done !== null && "files" in done) this.status = done as unknown as KnowledgeStatus;
      return done;
    } catch (error) {
      this.failure = messageOf(error);
      throw error;
    } finally {
      this.busy = false;
    }
  }

  private quietly<T>(task: () => Promise<T>) {
    return this.run(task).catch(() => null);
  }

  async load() {
    this.policy ??= await this.backend.privacyPolicy().catch(() => null);
    return this.quietly(() => this.backend.knowledgeStatus());
  }

  private turnedOn() {
    this.settings.set("privacyVeil", true);
    this.settings.set("knowledgeOn", true);
  }

  async create(name: string, password: string, remember: boolean) {
    await this.run(() => this.backend.knowledgeCreate(name, password, remember));
    this.turnedOn();
  }

  pick() {
    return this.run(() => this.backend.knowledgePick());
  }

  async import(password: string, remember: boolean) {
    await this.run(() => this.backend.knowledgeImport(password, remember));
    this.turnedOn();
  }

  unlock(password: string, remember: boolean) {
    const id = this.status?.active;
    if (id == null) return Promise.resolve(null);
    return this.run(() => this.backend.knowledgeUnlock(id, password, remember));
  }

  activate(id: string | null) {
    return this.quietly(() => this.backend.knowledgeActivate(id));
  }

  export(password: string) {
    return this.run(() => this.backend.knowledgeExport(password));
  }

  changePassword(current: string, password: string, remember: boolean) {
    return this.run(() => this.backend.knowledgePassword(current, password, remember));
  }

  remove(id: string) {
    return this.quietly(() => this.backend.knowledgeRemove(id));
  }

  importGlossary() {
    return this.quietly(() => this.backend.glossaryImport());
  }

  add(path: string) {
    return this.quietly(() => this.backend.knowledgeAdd(path));
  }

  attach(chat: string, path: string) {
    return this.quietly(() => this.backend.knowledgeAttach(chat, path));
  }

  forget(project: string) {
    return this.quietly(() => this.backend.knowledgeForget(project));
  }

  rebuild() {
    return this.quietly(() => this.backend.knowledgeRebuild());
  }

  async download(model: ModelId) {
    if (this.progress[model] !== undefined) return;
    this.progress[model] = { done: 0, total: 0 };
    this.failure = null;
    try {
      await this.backend.modelsDownload(model, (step) => (this.progress[model] = step));
      if (model === "names") this.settings.set("privacyNames", true);
      await this.load();
    } catch (error) {
      this.failure = messageOf(error);
    } finally {
      delete this.progress[model];
    }
  }

  cancel(model: ModelId) {
    return this.backend.modelsCancel(model);
  }

  pageLevel(page: string) {
    return Math.max(this.settings.values.pageLevels[page] ?? 1, this.policy?.minimumLevel ?? 0);
  }

  setPageLevel(page: string, level: number) {
    this.settings.set("pageLevels", { ...this.settings.values.pageLevels, [page]: level });
  }

  view(text: string, level?: number) {
    return this.run(() => this.backend.privacyView(text, level));
  }

  async override(text: string, kind: string | null) {
    await this.run(() => this.backend.privacyOverride(text, kind));
    this.changes += 1;
  }
}

export const privacy = new Privacy(ipc, settings);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;
  const { defaults } = await import("./settings.svelte");
  const detection = (await import("../../../src-tauri/resources/detection.json")).default;

  test("every kind znote can find can also be chosen by hand", () => {
    const found = [...detection.patterns.map((pattern) => pattern.kind), ...detection.finder.labels.map(([, kind]) => kind)];
    expect(found.filter((kind) => !kinds.includes(kind))).toEqual([]);
  });

  test("every kind has a colour of its own", () => {
    expect(new Set(kinds.map(kindColour)).size).toBe(kinds.length);
    expect(kinds.map(kindColour)).not.toContain(kindColour(null));
  });

  const status = (changes: Partial<KnowledgeStatus> = {}): KnowledgeStatus => ({
    files: [{ id: "0123456789abcdef", name: "Work" }],
    active: "0123456789abcdef",
    open: true,
    remembered: true,
    concepts: 3,
    glossary: true,
    hidden: 0,
    names: false,
    search: false,
    projects: [],
    ...changes,
  });

  test("hiding follows the wish and the policy, and says what still stops it", () => {
    const wanted = { ...defaults, privacyVeil: true, knowledgeOn: true, privacyNames: true };
    expect(optionsOf(wanted, null)).toEqual({ veil: true, amounts: true, names: true, knowledge: false, autoAdd: true });
    expect(optionsOf(wanted, status({ active: null }))).toMatchObject({ veil: true, knowledge: false });
    expect(optionsOf(wanted, status({ open: false }))).toMatchObject({ veil: true, knowledge: true, names: true });
    expect(optionsOf(defaults, status())).toMatchObject({ veil: false, knowledge: false });
    expect(blockedBy(optionsOf(wanted, null), null)).toBe("noFile");
    expect(blockedBy(optionsOf(wanted, status({ open: false })), status({ open: false }))).toBe("locked");
    expect(blockedBy(optionsOf(wanted, status()), status())).toBe("noFinder");
    expect(blockedBy(optionsOf(wanted, status({ names: true })), status({ names: true }))).toBe(null);
    expect(blockedBy(optionsOf(defaults, null), null)).toBe(null);
    const policy: PrivacyPolicy = { hideNames: "required", findNames: "optional", hideAmounts: "required", minimumLevel: 2, web: "blocked", providers: ["custom"], dataCollection: "deny", zeroRetention: true };
    expect(optionsOf({ ...defaults, privacyAmounts: false }, status(), policy)).toMatchObject({ veil: true, amounts: true, names: false });
  });

  test("a new knowledge file turns hiding on, and a failure is shown without losing the state", async () => {
    const chosen = new SettingsState(() => Promise.resolve());
    const backend = {
      knowledgeCreate: vi.fn(async () => status()),
      knowledgeUnlock: vi.fn(async () => {
        throw new Error("The password is wrong, or this is not a knowledge file.");
      }),
      modelsDownload: vi.fn(async (_model: ModelId, step: (progress: Progress) => void) => step({ done: 5, total: 10 })),
      knowledgeStatus: vi.fn(async () => status({ names: true })),
    } as unknown as Backend;
    const state = new Privacy(backend, chosen);
    await state.create("Work", "long enough", true);
    expect(state.open).toBe(true);
    expect(chosen.values.privacyVeil && chosen.values.knowledgeOn).toBe(true);
    expect(state.hiding).toBe(true);
    await expect(state.unlock("nope", false)).rejects.toThrow();
    expect(state.failure).toBe("The password is wrong, or this is not a knowledge file.");
    expect(state.status?.open).toBe(true);
    await state.download("names");
    expect(chosen.values.privacyNames).toBe(true);
    expect(state.options.names).toBe(true);
    expect(state.progress).toEqual({});
  });

  test("a page without its own level is light, and a new level keeps the others", () => {
    const chosen = new SettingsState(() => Promise.resolve());
    const state = new Privacy({} as Backend, chosen);
    expect(state.pageLevel("Work/Notes/A")).toBe(1);
    state.setPageLevel("Work/Notes/A", 3);
    state.setPageLevel("Work/Notes/B", 0);
    expect(chosen.values.pageLevels).toEqual({ "Work/Notes/A": 3, "Work/Notes/B": 0 });
    expect(state.pageLevel("Work/Notes/B")).toBe(0);
  });

  test("a selection is hidden line by line, without blanks, repeats or very long lines", () => {
    expect(linesToHide("  Acme  ")).toEqual(["Acme"]);
    expect(linesToHide("Acme\n\n Falcon \nAcme\n" + "x".repeat(201))).toEqual(["Acme", "Falcon"]);
    expect(linesToHide("")).toEqual([]);
  });

  test("hidden counts add up and only bracketed tokens look like tokens", () => {
    expect(hiddenCount({ CUSTOMER: 3, EMAIL: 2 })).toBe(5);
    expect("⟦CUSTOMER_1⟧ and ⟦X⟧ but not [CUSTOMER_1]".match(tokens())).toEqual(["⟦CUSTOMER_1⟧", "⟦X⟧"]);
  });

  test("a saved choice tells every open view to refresh, and a failed one does not", async () => {
    const privacyOverride = vi.fn(async () => {});
    const state = new Privacy({ privacyOverride } as unknown as Backend, new SettingsState(() => Promise.resolve()));
    await state.override("Falcon", "PROJECT");
    expect(state.changes).toBe(1);
    privacyOverride.mockRejectedValueOnce(new Error("locked"));
    await expect(state.override("Falcon", null)).rejects.toThrow();
    expect(state.changes).toBe(1);
  });
}
