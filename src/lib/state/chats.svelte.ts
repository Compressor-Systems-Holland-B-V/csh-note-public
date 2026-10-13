import { readChats, writeChats } from "$lib/ipc";
import type { StoredChat } from "$lib/ipc";

type Persist = (chats: StoredChat[]) => Promise<void>;

export function stored(value: unknown): StoredChat[] {
  if (!Array.isArray(value)) return [];
  return value.filter(
    (chat): chat is StoredChat =>
      typeof chat === "object" &&
      chat !== null &&
      typeof chat.id === "string" &&
      typeof chat.title === "string" &&
      Array.isArray(chat.messages) &&
      chat.messages.every((message: unknown) => typeof message === "object" && message !== null && "parts" in message && Array.isArray(message.parts)),
  );
}

export class Chats {
  list = $state<StoredChat[]>([]);
  failure = $state<string | null>(null);

  constructor(private persist: Persist) {}

  async load(read: () => Promise<unknown>) {
    this.list = stored(await read());
  }

  private keep(list: StoredChat[]) {
    this.list = list.toSorted((first, second) => second.updated - first.updated);
    this.persist($state.snapshot(this.list)).then(
      () => (this.failure = null),
      (error: unknown) => (this.failure = `Your chats could not be saved: ${String(error)}`),
    );
  }

  save(chat: StoredChat) {
    this.keep([chat, ...this.list.filter(({ id }) => id !== chat.id)]);
  }

  archive(id: string, archived: boolean) {
    this.keep(this.list.map((chat) => (chat.id === id ? { ...chat, archived } : chat)));
  }

  remove(id: string) {
    this.keep(this.list.filter((chat) => chat.id !== id));
  }
}

export const chats = new Chats(writeChats);

export const loadChats = () => chats.load(readChats).catch((error: unknown) => (chats.failure = String(error)));

if (import.meta.vitest) {
  const { test, expect, vi } = import.meta.vitest;

  const chat = (id: string, updated: number): StoredChat => ({ id, title: id, session: null, messages: [], archived: false, updated });

  test("chats are kept newest first, and saving one again replaces it", () => {
    const persist = vi.fn<Persist>(() => Promise.resolve());
    const chats = new Chats(persist);
    chats.save(chat("a", 1));
    chats.save(chat("b", 2));
    chats.save({ ...chat("a", 3), title: "renamed" });
    expect(chats.list.map(({ id, title }) => [id, title])).toEqual([["a", "renamed"], ["b", "b"]]);
    chats.archive("b", true);
    expect(chats.list[1].archived).toBe(true);
    chats.remove("a");
    expect(persist.mock.lastCall?.[0].map(({ id }) => id)).toEqual(["b"]);
  });

  test("a damaged chat file is read as far as it makes sense", () => {
    expect(stored(null)).toEqual([]);
    expect(stored("chats")).toEqual([]);
    expect(stored([chat("a", 1), { id: 7 }, null, { id: "b", title: "b", messages: [{ from: "you" }] }])).toEqual([chat("a", 1)]);
  });
}
