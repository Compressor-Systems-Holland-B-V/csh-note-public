<script lang="ts">
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import type { Piece } from "$lib/ipc";
  import { kindColour, kinds, privacy } from "$lib/state/privacy.svelte";
  import { messageOf } from "$lib/utils";
  import { t } from "$lib/i18n.svelte";

  let { text, level }: { text: string; level?: number } = $props();

  let pieces = $state<Piece[]>([]);
  let chosen = $state<Piece | null>(null);
  let selected = $state("");
  let failure = $state<string | null>(null);

  const kindName = (kind: string | null) => (kind !== null && kinds.includes(kind) ? t(`privacy.kinds.${kind}`) : (kind ?? ""));

  $effect(() => {
    void privacy.changes;
    const [current, wanted] = [text, level];
    privacy.view(current, wanted).then(
      (found) => {
        if (current === text && wanted === level) pieces = found;
      },
      (error: unknown) => (failure = messageOf(error)),
    );
  });

  function noted(event: MouseEvent) {
    const index = event.target instanceof Element ? event.target.closest("[data-piece]")?.getAttribute("data-piece") : null;
    chosen = index == null ? null : pieces[Number(index)];
    const picked = window.getSelection()?.toString() ?? "";
    selected = chosen || picked.includes("\n") || picked.length > 200 ? "" : picked.trim();
  }

  async function change(text: string, kind: string | null) {
    failure = null;
    try {
      await privacy.override(text, kind);
    } catch (error) {
      failure = messageOf(error);
    }
  }
</script>

<div class="flex h-full flex-col" aria-label={t("privacy.view.title")} oncontextmenucapture={noted}>
  <p class="shrink-0 border-b px-3 py-1.5 text-xs text-muted-foreground">{t("privacy.view.hint")}</p>
  {#if failure}<p class="shrink-0 px-3 py-1 text-sm text-destructive" role="alert">{failure}</p>{/if}
  <ContextMenu.Root>
    <ContextMenu.Trigger>
      {#snippet child({ props })}
        <pre {...props} class="min-h-0 flex-1 overflow-auto p-3 font-mono text-sm wrap-anywhere whitespace-pre-wrap" dir="ltr">{#each pieces as piece, index (index)}{#if piece.token}<button
                type="button"
                data-piece={index}
                class="rounded-sm px-0.5 outline-none hover:ring-1 hover:ring-ring focus-visible:ring-2 focus-visible:ring-ring {kindColour(piece.kind)}"
                title={`${piece.text} (${kindName(piece.kind)})`}>{piece.token}</button>{:else}{piece.text}{/if}{/each}</pre>
      {/snippet}
    </ContextMenu.Trigger>
    <ContextMenu.Content class="max-w-80">
      {#if chosen}
        {@const piece = chosen}
        <ContextMenu.Label class="truncate">{t("privacy.view.isHidden", { text: piece.text, kind: kindName(piece.kind) })}</ContextMenu.Label>
        <ContextMenu.Item onSelect={() => change(piece.text, null)}>{t("privacy.view.dontHide")}</ContextMenu.Item>
      {:else if selected}
        {@const picked = selected}
        <ContextMenu.Sub>
          <ContextMenu.SubTrigger><span class="truncate">{t("privacy.view.hideAs", { text: picked })}</span></ContextMenu.SubTrigger>
          <ContextMenu.SubContent>
            {#each kinds as kind (kind)}
              <ContextMenu.Item onSelect={() => change(picked, kind)}><span class="size-3 rounded-sm {kindColour(kind)}"></span> {kindName(kind)}</ContextMenu.Item>
            {/each}
          </ContextMenu.SubContent>
        </ContextMenu.Sub>
      {:else}
        <ContextMenu.Label>{t("privacy.view.hint")}</ContextMenu.Label>
      {/if}
    </ContextMenu.Content>
  </ContextMenu.Root>
</div>
