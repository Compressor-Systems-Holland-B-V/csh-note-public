import { translated } from "$lib/i18n.svelte";
export { cn, type WithElementRef, type WithoutChild, type WithoutChildren, type WithoutChildrenOrChild } from "@zsuite/ui/utils";

export const messageOf = (error: unknown) => translated(error instanceof Error ? error.message : String(error));
