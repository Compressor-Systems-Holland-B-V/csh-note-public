import { forgetOldChats, readChats, readOldChats, writeChat } from "$lib/ipc";
import type { StoredChat } from "$lib/ipc";
import { messageOf } from "$lib/utils";
import { t } from "$lib/i18n.svelte";

type Persist = (chat: StoredChat) => Promise<void>;

export function stored(value: unknown): StoredChat[] {
  if (!Array.isArray(value)) return [];
  const known = new Set<string>();
  return value.flatMap((chat): StoredChat[] => {
    const sound =
      typeof chat === "object" &&
      chat !== null &&
      typeof chat.id === "string" &&
      typeof chat.title === "string" &&
      Array.isArray(chat.messages) &&
      chat.messages.every((message: unknown) => typeof message === "object" && message !== null && "parts" in message && Array.isArray(message.parts));
    if (!sound || known.has(chat.id)) return [];
    known.add(chat.id);
    return [
      {
        id: chat.id,
        title: chat.title,
        messages: chat.messages,
        session: typeof chat.session === "string" ? chat.session : null,
        provider: typeof chat.provider === "string" ? chat.provider : chat.session ? "claude" : null,
        archived: chat.archived === true,
        updated: typeof chat.updated === "number" ? chat.updated : 0,
      },
    ];
  });
}

export class Chats {
  list = $state<StoredChat[]>([]);
  failure = $state<string | null>(null);

  constructor(private persist: Persist) {}

  async load(read: () => Promise<unknown>, readOld: () => Promise<unknown>, moved: () => Promise<unknown>) {
    const saved = stored(await read());
    const old = stored([...saved, ...stored(await readOld())]).slice(saved.length);
    this.list = [...saved, ...old].toSorted((first, second) => second.updated - first.updated);
    if (old.length === 0) return;
    await Promise.all(old.map((chat) => this.persist(chat)));
    await moved();
  }

  private keep(list: StoredChat[], changed: StoredChat | undefined) {
    this.list = list.toSorted((first, second) => second.updated - first.updated);
    if (changed === undefined) return;
    this.persist($state.snapshot(changed) as StoredChat).then(
      () => (this.failure = null),
      (error: unknown) => (this.failure = t("assistant.chatSaveFailed", { reason: messageOf(error) })),
    );
  }

  save(chat: StoredChat) {
    this.keep([chat, ...this.list.filter(({ id }) => id !== chat.id)], chat);
  }

  archive(id: string, archived: boolean) {
    const list = this.list.map((chat) => (chat.id === id ? { ...chat, archived } : chat));
    this.keep(list, list.find((chat) => chat.id === id));
  }

  remove(id: string) {
    this.keep(this.list.filter((chat) => chat.id !== id), undefined);
  }
}

export const chats = new Chats(writeChat);

export const loadChats = () => chats.load(readChats, readOldChats, forgetOldChats).catch((error: unknown) => (chats.failure = messageOf(error)));

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  const chat = (id: string, updated: number): StoredChat => ({ id, title: id, session: null, provider: null, messages: [], archived: false, updated });

  test("chats are kept newest first, and only the chat that changed is written", () => {
    const persist = vi.fn<Persist>(() => Promise.resolve());
    const chats = new Chats(persist);
    chats.save(chat("a", 1));
    chats.save(chat("b", 2));
    chats.save({ ...chat("a", 3), title: "renamed" });
    expect(chats.list.map(({ id, title }) => [id, title])).toEqual([["a", "renamed"], ["b", "b"]]);
    chats.archive("b", true);
    expect(chats.list[1].archived).toBe(true);
    expect(persist.mock.calls.map(([saved]) => saved.id)).toEqual(["a", "b", "a", "b"]);
    chats.remove("a");
    expect(chats.list.map(({ id }) => id)).toEqual(["b"]);
    expect(persist).toHaveBeenCalledTimes(4);
  });

  test("a damaged chat is read as far as it makes sense, and one saved twice is kept once", () => {
    expect(stored(null)).toEqual([]);
    expect(stored("chats")).toEqual([]);
    expect(stored([chat("a", 1), { id: 7 }, null, undefined, { id: "b", title: "b", messages: [{ from: "you" }] }, chat("a", 9)])).toEqual([chat("a", 1)]);
    expect(stored([{ id: "old", title: "old", messages: [], session: "s-1" }])).toEqual([{ ...chat("old", 0), session: "s-1", provider: "claude" }]);
  });

  test("chats from the old single file are moved to their own files once", async () => {
    const persist = vi.fn<Persist>(() => Promise.resolve());
    const moved = vi.fn(() => Promise.resolve());
    const chats = new Chats(persist);
    await chats.load(async () => [chat("new", 5)], async () => [chat("old", 2), chat("new", 1)], moved);
    expect(chats.list.map(({ id }) => id)).toEqual(["new", "old"]);
    expect(persist.mock.calls.map(([saved]) => saved.id)).toEqual(["old"]);
    expect(moved).toHaveBeenCalledTimes(1);
    await chats.load(async () => [chat("new", 5), chat("old", 2)], async () => null, moved);
    expect(persist).toHaveBeenCalledTimes(1);
    expect(moved).toHaveBeenCalledTimes(1);
  });
}
