import * as ipc from "$lib/ipc";
import type { KnowledgeStatus, ModelId, PrivacyOptions, PrivacyPolicy, Progress } from "$lib/ipc";
import { tokens } from "$lib/markdown";
import { messageOf } from "$lib/utils";
import { SettingsState, settings } from "./settings.svelte";
import type { Settings } from "./settings.svelte";
import { kinds, kindColour, optionsOf, blockedBy, linesToHide, hiddenCount, Privacy, type Blocked, type Backend } from "@zsuite/assistant/state/privacy";

export * from "@zsuite/assistant/state/privacy";

export const privacy = new Privacy(ipc, settings);

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;
  const { defaults } = await import("./settings.svelte");
  const detection = (await import("../../../vendor/zsuite/crates/zsuite-veil/resources/detection.json")).default;

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
