<script lang="ts">
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import { Button } from "@zsuite/ui/button";
  import { Input } from "@zsuite/ui/input";
  import * as Resizable from "@zsuite/ui/resizable";
  import type { Piece } from "$lib/ipc";
  import { doc } from "$lib/state/document.svelte";
  import { kindColour, kinds, privacy } from "$lib/state/privacy.svelte";
  import { groupsOf, hideChoice, itemsOf, redaction, remember, unwanted, type Item, type Known } from "$lib/state/redaction.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { messageOf } from "$lib/utils";
  import LevelSlider from "./LevelSlider.svelte";
  import PrivacyView from "./PrivacyView.svelte";
  import { t } from "$lib/i18n.svelte";

  let { onsettings }: { onsettings: () => void } = $props();

  let panel = $state<HTMLElement>();
  let view = $state<{ page: string; pieces: Piece[] }>({ page: "", pieces: [] });
  let failure = $state<string | null>(null);
  let filter = $state("");
  const memory = new Map<string, Known>();

  const page = $derived(workspace.page);
  const level = $derived(page === null ? 1 : privacy.pageLevel(page));
  const ready = $derived(page !== null && privacy.hiding && privacy.open);
  const items = $derived(view.page === page ? itemsOf(memory.get(view.page) ?? new Map(), view.pieces, redaction.overrides, doc.text) : []);
  const wanted = $derived(filter.trim().toLowerCase());
  const groups = $derived(groupsOf(items.filter((item) => item.text.toLowerCase().includes(wanted)), kinds));
  const hiddenCount = $derived(items.filter((item) => item.hidden).length);
  const removable = $derived(unwanted(items));

  function removeUnhidden() {
    const texts = removable.map((item) => item.text);
    for (const text of texts) memory.get(view.page)?.delete(text);
    view = { ...view };
    run(() => redaction.reset(texts));
  }

  const kindName = (kind: string) => (kinds.includes(kind) ? t(`privacy.kinds.${kind}`) : kind === "OTHER" ? t("privacy.redact.other") : kind.charAt(0) + kind.slice(1).toLowerCase());

  $effect(() => {
    if (ready) redaction.load().catch((error: unknown) => (failure = messageOf(error)));
  });

  $effect(() => {
    void privacy.changes;
    void doc.opened;
    const [shown, text, wantedLevel] = [doc.page, doc.text, level];
    if (!ready || shown !== page) return;
    privacy.view(text, wantedLevel).then(
      (found) => {
        if (doc.page !== shown || doc.text !== text) return;
        const known = memory.get(shown) ?? new Map();
        memory.set(shown, known);
        remember(known, found, (name) => redaction.choiceOf(name));
        view = { page: shown, pieces: found };
      },
      (error: unknown) => (failure = messageOf(error)),
    );
  });

  async function run(task: () => Promise<void>) {
    failure = null;
    try {
      await task();
    } catch (error) {
      failure = messageOf(error);
    }
  }

  const setHidden = (list: Item[], hide: boolean) =>
    run(() => redaction.set(list.filter((item) => item.hidden !== hide).map((item) => ({ text: item.text, after: hide ? hideChoice(item) : null }))));

  function keys(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || !(event.target instanceof Node) || !panel?.contains(event.target) || event.target instanceof HTMLInputElement) return;
    const key = event.key.toLowerCase();
    if (key === "z" && !event.shiftKey) run(() => redaction.undo());
    else if (key === "y" || (key === "z" && event.shiftKey)) run(() => redaction.redo());
    else return;
    event.preventDefault();
  }
</script>

<svelte:window onkeydown={keys} />

{#snippet tool(label: string, Icon: typeof Undo2, onclick: () => void, disabled: boolean)}
  <Button variant="ghost" size="icon-xs" aria-label={label} title={label} {disabled} {onclick}><Icon /></Button>
{/snippet}

<section bind:this={panel} class="flex h-full flex-col text-sm" aria-label={t("privacy.redact.title")}>
  <div class="flex h-9 shrink-0 items-center gap-1 border-b px-3">
    <span class="font-medium">{t("privacy.redact.title")}</span>
    <span class="min-w-0 flex-1 truncate text-muted-foreground">{page ?? ""}</span>
    {#if ready}
      {@render tool(t("privacy.redact.undo"), Undo2, () => run(() => redaction.undo()), redaction.done.length === 0)}
      {@render tool(t("privacy.redact.redo"), Redo2, () => run(() => redaction.redo()), redaction.undone.length === 0)}
      {@render tool(t("privacy.redact.reset"), RotateCcw, () => run(() => redaction.reset(items.map((item) => item.text))), !items.some((item) => redaction.choiceOf(item.text) !== undefined))}
      {@render tool(t("privacy.redact.removeUnhidden"), Trash2, removeUnhidden, removable.length === 0)}
    {/if}
  </div>
  {#if page === null}
    <p class="flex flex-1 items-center justify-center p-4 text-center text-muted-foreground">{t("workspace.placeholder")}</p>
  {:else if !privacy.hiding || !privacy.open}
    <div class="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center text-muted-foreground">
      <p>{t(privacy.hiding ? "privacy.panel.lockedTitle" : "privacy.redact.off")}</p>
      <Button variant="outline" size="sm" onclick={onsettings}>{t(privacy.hiding ? "common.settings" : "privacy.redact.turnOn")}</Button>
    </div>
  {:else}
    {@const current = page}
    <LevelSlider class="shrink-0 border-b px-3 py-2" {level} onlevel={(chosen) => privacy.setPageLevel(current, chosen)} />
    <Resizable.PaneGroup direction="vertical" autoSaveId="znote-redaction" class="min-h-0 flex-1">
      <Resizable.Pane defaultSize={40} minSize={15} class="flex min-h-0 flex-col">
        <div class="flex shrink-0 items-center gap-2 px-3 py-2">
          <Input type="search" class="h-7 min-w-0 flex-1" placeholder={t("privacy.redact.filter")} aria-label={t("privacy.redact.filter")} bind:value={filter} />
          <span class="shrink-0 text-xs text-muted-foreground tabular-nums">{t("privacy.redact.count", { hidden: hiddenCount, total: items.length })}</span>
        </div>
        {#if failure}<p class="px-3 text-destructive" role="alert">{failure}</p>{/if}
        <div class="min-h-0 flex-1 overflow-y-auto px-3 pb-2">
          {#each groups as group (group.kind)}
            {@const on = group.items.filter((item) => item.hidden).length}
            <fieldset class="mb-2">
              <legend class="flex w-full items-center gap-2 py-1 font-medium">
                <input
                  type="checkbox"
                  class="accent-primary"
                  checked={on === group.items.length}
                  indeterminate={on > 0 && on < group.items.length}
                  aria-label={t("privacy.redact.hideGroup", { kind: kindName(group.kind) })}
                  onchange={() => setHidden(group.items, on < group.items.length)}
                />
                <span class="size-3 shrink-0 rounded-sm {kindColour(group.kind)}"></span>
                {kindName(group.kind)}
                <span class="text-xs font-normal text-muted-foreground tabular-nums">{on}/{group.items.length}</span>
              </legend>
              {#each group.items as item (item.text)}
                <label class="flex cursor-pointer items-center gap-2 rounded-md py-0.5 ps-5 pe-1 hover:bg-accent">
                  <input type="checkbox" class="accent-primary" checked={item.hidden} onchange={() => setHidden([item], !item.hidden)} />
                  <span class="min-w-0 flex-1 truncate {item.hidden ? '' : 'text-muted-foreground'}" title={item.text}>{item.text}</span>
                </label>
              {/each}
            </fieldset>
          {:else}
            <p class="py-2 text-muted-foreground">{t("privacy.redact.none")}</p>
          {/each}
        </div>
      </Resizable.Pane>
      <Resizable.Handle withHandle />
      <Resizable.Pane defaultSize={60} minSize={15}>
        <PrivacyView text={doc.text} {level} onchoose={(texts, kind) => redaction.set(texts.map((text) => ({ text, after: kind })))} />
      </Resizable.Pane>
    </Resizable.PaneGroup>
  {/if}
</section>
