import { readChats, writeChat } from "$lib/ipc";
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
        provider: typeof chat.provider === "string" ? chat.provider : null,
        archived: chat.archived === true,
        updated: typeof chat.updated === "number" ? chat.updated : 0,
        knowledge: typeof chat.knowledge === "string" ? chat.knowledge : null,
      },
    ];
  });
}

export class Chats {
  list = $state<StoredChat[]>([]);
  failure = $state<string | null>(null);

  constructor(private persist: Persist) {}

  async load(read: () => Promise<unknown>) {
    this.list = stored(await read()).toSorted((first, second) => second.updated - first.updated);
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

export const loadChats = () => chats.load(readChats).catch((error: unknown) => (chats.failure = messageOf(error)));

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  const chat = (id: string, updated: number): StoredChat => ({ id, title: id, session: null, provider: null, messages: [], archived: false, updated, knowledge: null });

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
    expect(stored([{ id: "old", title: "old", messages: [], session: "s-1" }])).toEqual([{ ...chat("old", 0), session: "s-1", provider: null }]);
    expect(stored([{ ...chat("hidden", 1), knowledge: "file-1" }])[0].knowledge).toBe("file-1");
  });

}
