import * as ipc from "$lib/ipc";
import type { Override, Piece } from "$lib/ipc";
import { privacy } from "./privacy.svelte";

type Backend = Pick<typeof ipc, "privacyOverride" | "privacyForget" | "privacyOverrides">;

export type Choice = string | null | undefined;
export type Change = { text: string; before: Choice; after: Choice };
export type Item = { text: string; kind: string; hidden: boolean; auto: boolean };
export type Known = Map<string, { kind: string; auto: boolean }>;
export type Group = { kind: string; items: Item[] };

const other = "OTHER";

export function remember(known: Known, pieces: Piece[], choiceOf: (text: string) => Choice) {
  for (const { text, token, kind } of pieces) {
    if (!token) continue;
    const auto = choiceOf(text) === undefined;
    const before = known.get(text);
    known.set(text, { kind: kind ?? before?.kind ?? other, auto: auto || (before?.auto ?? false) });
  }
}

const escaped = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export function itemsOf(known: Known, pieces: Piece[], overrides: Override[], text: string): Item[] {
  const tokened = new Set(pieces.filter((piece) => piece.token).map((piece) => piece.text));
  const plain = pieces.filter((piece) => !piece.token).map((piece) => piece.text);
  // A name counts as hidden only when no place on the page still shows it.
  const hidden = (name: string) => {
    const word = new RegExp(`(?<![\\p{L}\\p{N}])${escaped(name)}(?![\\p{L}\\p{N}])`, "iu");
    return tokened.has(name) && !plain.some((piece) => word.test(piece));
  };
  const items = new Map<string, Item>();
  for (const [name, { kind, auto }] of known) if (text.includes(name)) items.set(name, { text: name, kind, hidden: hidden(name), auto });
  for (const piece of pieces)
    if (piece.token && !items.has(piece.text)) items.set(piece.text, { text: piece.text, kind: piece.kind ?? other, hidden: hidden(piece.text), auto: false });
  for (const { text: name, kind } of overrides)
    if (!items.has(name) && text.includes(name)) items.set(name, { text: name, kind: kind ?? other, hidden: hidden(name), auto: false });
  return [...items.values()];
}

export function groupsOf(items: Item[], order: string[]): Group[] {
  const groups = Map.groupBy(items, (item) => item.kind);
  const rank = (kind: string) => (kind === other ? order.length + 1 : order.includes(kind) ? order.indexOf(kind) : order.length);
  return [...groups]
    .sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
    .map(([kind, list]) => ({ kind, items: list.sort((a, b) => a.text.localeCompare(b.text)) }));
}

export const unwanted = (items: Item[]) => items.filter((item) => !item.hidden && !item.auto);

export const hideChoice = (item: Item): Choice => (item.auto ? undefined : item.kind === other ? "ORGANISATION" : item.kind);

export class Redaction {
  done = $state<Change[][]>([]);
  undone = $state<Change[][]>([]);
  overrides = $state<Override[]>([]);

  constructor(
    private backend: Backend,
    private changed: () => void = () => {},
  ) {}

  choiceOf(text: string): Choice {
    const found = this.overrides.find((entry) => entry.text === text);
    return found === undefined ? undefined : found.kind;
  }

  async load() {
    this.overrides = await this.backend.privacyOverrides();
  }

  private async put(changes: Change[], to: "before" | "after") {
    for (const change of changes) {
      const choice = change[to];
      if (choice === undefined) await this.backend.privacyForget(change.text);
      else await this.backend.privacyOverride(change.text, choice);
    }
    await this.load();
    this.changed();
  }

  async set(entries: { text: string; after: Choice }[]) {
    const changes = entries.map(({ text, after }) => ({ text, before: this.choiceOf(text), after })).filter(({ before, after }) => before !== after);
    if (changes.length === 0) return;
    await this.put(changes, "after");
    this.done.push(changes);
    this.undone = [];
  }

  reset(texts: string[]) {
    return this.set(texts.map((text) => ({ text, after: undefined })));
  }

  async undo() {
    const changes = this.done.pop();
    if (!changes) return;
    await this.put([...changes].reverse(), "before");
    this.undone.push(changes);
  }

  async redo() {
    const changes = this.undone.pop();
    if (!changes) return;
    await this.put(changes, "after");
    this.done.push(changes);
  }

  clear() {
    this.done = [];
    this.undone = [];
  }
}

export const redaction = new Redaction(ipc, () => (privacy.changes += 1));

if (import.meta.vitest) {
  const { test, expect } = import.meta.vitest;

  function fake() {
    const stored = new Map<string, string | null>();
    const backend = {
      privacyOverride: async (text: string, kind: string | null) => void stored.set(text, kind),
      privacyForget: async (text: string) => void stored.delete(text),
      privacyOverrides: async () => [...stored].map(([text, kind]) => ({ text, kind })),
    } as unknown as Backend;
    return { stored, state: new Redaction(backend) };
  }

  test("a change can be undone back to no choice at all, and redone", async () => {
    const { stored, state } = fake();
    await state.set([{ text: "Acme", after: null }]);
    expect(stored.get("Acme")).toBeNull();
    await state.set([{ text: "Acme", after: "CUSTOMER" }]);
    await state.undo();
    expect(stored.get("Acme")).toBeNull();
    await state.undo();
    expect(stored.has("Acme")).toBe(false);
    await state.redo();
    await state.redo();
    expect(stored.get("Acme")).toBe("CUSTOMER");
    expect(state.undone).toEqual([]);
  });

  test("a reset is one step, skips what has no choice, and a new change drops the redo list", async () => {
    const { stored, state } = fake();
    await state.set([{ text: "Acme", after: null }, { text: "Falcon", after: "PROJECT" }]);
    await state.reset(["Acme", "Falcon", "Nobody"]);
    expect(stored.size).toBe(0);
    expect(state.done.at(-1)).toHaveLength(2);
    await state.undo();
    expect(stored.get("Falcon")).toBe("PROJECT");
    await state.set([{ text: "Acme", after: "CUSTOMER" }]);
    expect(state.undone).toEqual([]);
  });

  test("only names on this page are listed, unhidden ones stay, and they are grouped by kind", () => {
    const known: Known = new Map();
    const piece = (text: string, kind: string | null = null) => ({ text, token: kind ? `⟦${kind}_1⟧` : null, kind });
    remember(known, [piece("Acme", "CUSTOMER"), piece("John", "PERSON"), piece("Bob", "PERSON")], () => undefined);
    const now = [piece("Acme met "), piece("John", "PERSON"), piece(" about Falcon and "), piece("x@y.z", "EMAIL")];
    const items = itemsOf(known, now, [{ text: "Falcon", kind: null }, { text: "Elsewhere", kind: "PROJECT" }], "Acme met John about Falcon and x@y.z");
    expect(items).toEqual([
      { text: "Acme", kind: "CUSTOMER", hidden: false, auto: true },
      { text: "John", kind: "PERSON", hidden: true, auto: true },
      { text: "x@y.z", kind: "EMAIL", hidden: true, auto: false },
      { text: "Falcon", kind: "OTHER", hidden: false, auto: false },
    ]);
    expect(groupsOf(items, ["CUSTOMER", "PERSON"]).map(({ kind }) => kind)).toEqual(["CUSTOMER", "PERSON", "EMAIL", "OTHER"]);
    expect(unwanted(items).map(({ text }) => text)).toEqual(["Falcon"]);
    expect(hideChoice(items[0])).toBeUndefined();
    expect(hideChoice(items[3])).toBe("ORGANISATION");
  });

  test("a name hidden in one place but shown in another is not listed as hidden", () => {
    const piece = (text: string, kind: string | null = null) => ({ text, token: kind ? `⟦${kind}_1⟧` : null, kind });
    const now = [piece("Marloes", "PERSON"), piece(" van den Berg. Bij Hendriks wil MARLOES, niet Marloesje; "), piece("Fatima", "PERSON")];
    const items = itemsOf(new Map(), now, [], "Marloes van den Berg. Bij Hendriks wil MARLOES, niet Marloesje; Fatima");
    expect(items.map(({ text, hidden }) => [text, hidden])).toEqual([
      ["Marloes", false],
      ["Fatima", true],
    ]);
  });
}
