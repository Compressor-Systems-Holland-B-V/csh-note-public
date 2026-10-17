import { readChats, writeChat } from "$lib/ipc";
import type { StoredChat } from "$lib/ipc";
import { messageOf } from "$lib/utils";
import { t } from "$lib/i18n.svelte";
import { stored, Chats } from "@zsuite/assistant/state/chats";

export * from "@zsuite/assistant/state/chats";

export const chats = new Chats(writeChat);

export const loadChats = () => chats.load(readChats).catch((error: unknown) => (chats.failure = messageOf(error)));
