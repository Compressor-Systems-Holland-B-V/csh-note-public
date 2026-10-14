<script lang="ts">
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import type { Piece } from "$lib/ipc";
  import { kindColour, kinds, linesToHide, privacy } from "$lib/state/privacy.svelte";
  import { messageOf } from "$lib/utils";
  import { t } from "$lib/i18n.svelte";
  import { piecesHtml } from "$lib/markdown";

  let {
    text,
    level,
    markdown = false,
    pieces = $bindable([]),
    onchoose,
  }: { text: string; level?: number; markdown?: boolean; pieces?: Piece[]; onchoose?: (texts: string[], kind: string | null) => Promise<void> } = $props();

  let chosen = $state<Piece | null>(null);
  let selected = $state<string[]>([]);
  let failure = $state<string | null>(null);

  const kindName = (kind: string | null) => (kind !== null && kinds.includes(kind) ? t(`privacy.kinds.${kind}`) : (kind ?? ""));
  const tokenClass = "rounded-sm px-0.5 outline-none hover:ring-1 hover:ring-ring focus-visible:ring-2 focus-visible:ring-ring";
  const tokenTitle = (piece: Piece) => `${piece.text} (${kindName(piece.kind)})`;
  const html = $derived(markdown ? piecesHtml(pieces, (piece) => ({ className: `${tokenClass} ${kindColour(piece.kind)}`, title: tokenTitle(piece) })) : "");

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
    selected = chosen ? [] : linesToHide(picked);
  }

  async function change(texts: string[], kind: string | null) {
    failure = null;
    try {
      if (onchoose) await onchoose(texts, kind);
      else for (const text of texts) await privacy.override(text, kind);
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
        {#if markdown}
          <div
            {...props}
            class="min-h-0 flex-1 overflow-auto p-3 text-sm wrap-anywhere [&_code]:font-mono [&_h1]:mt-4 [&_h1]:mb-2 [&_h1]:text-lg [&_h1]:font-semibold [&_h2]:mt-4 [&_h2]:mb-2 [&_h2]:border-b [&_h2]:pb-1 [&_h2]:font-semibold [&_h3]:mt-3 [&_h3]:mb-1 [&_h3]:font-medium [&_h4]:font-medium [&_h5]:font-medium [&_h6]:font-medium [&_li]:my-0.5 [&_ol]:my-2 [&_ol]:list-decimal [&_ol]:ps-5 [&_p]:my-2 [&_table]:my-2 [&_table]:border-collapse [&_td]:border [&_td]:px-2 [&_td]:py-1 [&_th]:border [&_th]:bg-muted [&_th]:px-2 [&_th]:py-1 [&_th]:text-start [&_th]:font-medium [&_ul]:my-2 [&_ul]:list-disc [&_ul]:ps-5"
            dir="ltr"
          >
            {@html html}
          </div>
        {:else}
          <pre {...props} class="min-h-0 flex-1 overflow-auto p-3 font-mono text-sm wrap-anywhere whitespace-pre-wrap" dir="ltr">{#each pieces as piece, index (index)}{#if piece.token}<button
                  type="button"
                  data-piece={index}
                  class="{tokenClass} {kindColour(piece.kind)}"
                  title={tokenTitle(piece)}>{piece.token}</button>{:else}{piece.text}{/if}{/each}</pre>
        {/if}
      {/snippet}
    </ContextMenu.Trigger>
    <ContextMenu.Content class="max-w-80">
      {#if chosen}
        {@const piece = chosen}
        <ContextMenu.Label class="truncate">{t("privacy.view.isHidden", { text: piece.text, kind: kindName(piece.kind) })}</ContextMenu.Label>
        <ContextMenu.Item onSelect={() => change([piece.text], null)}>{t("privacy.view.dontHide")}</ContextMenu.Item>
      {:else if selected.length > 0}
        {@const picked = selected}
        <ContextMenu.Sub>
          <ContextMenu.SubTrigger><span class="truncate">{t("privacy.view.hideAs", { text: picked.length > 1 ? t("privacy.view.lines", { count: picked.length }) : picked[0] })}</span></ContextMenu.SubTrigger>
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
