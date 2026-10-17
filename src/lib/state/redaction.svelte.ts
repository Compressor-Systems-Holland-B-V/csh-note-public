import * as ipc from "$lib/ipc";
import type { Override, Piece } from "$lib/ipc";
import { privacy } from "./privacy.svelte";
import { remember, itemsOf, groupsOf, unwanted, hideChoice, Redaction, Steps, type Choice, type Change, type Item, type Known, type Group, type Backend } from "@zsuite/assistant/state/redaction";

export * from "@zsuite/assistant/state/redaction";

export const redaction = new Redaction(ipc, () => (privacy.changes += 1));
