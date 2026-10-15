<script lang="ts">
  import FileText from "@lucide/svelte/icons/file-text";
  import Search from "@lucide/svelte/icons/search";
  import { Input } from "@zsuite/ui/input";
  import { searchPages, type Hit } from "$lib/ipc";
  import { workspace } from "$lib/state/workspace.svelte";
  import { messageOf } from "$lib/utils";
  import WorkspaceBrowser from "./WorkspaceBrowser.svelte";
  import { t } from "$lib/i18n.svelte";

  let query = $state("");
  let hits = $state<Hit[]>([]);
  let failure = $state<string | null>(null);

  const found = $derived(Map.groupBy(hits, (hit) => hit.page));

  $effect(() => {
    const wanted = query.trim();
    if (wanted === "") return;
    const timer = setTimeout(() => {
      searchPages(wanted).then(
        (result) => {
          if (query.trim() === wanted) [hits, failure] = [result, null];
        },
        (error: unknown) => (failure = messageOf(error)),
      );
    }, 200);
    return () => clearTimeout(timer);
  });
</script>

<div class="flex h-full flex-col">
  <div class="relative shrink-0 border-b p-2">
    <Search class="pointer-events-none absolute start-4 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
    <Input type="search" class="ps-8" placeholder={t("workspace.search")} aria-label={t("workspace.search")} bind:value={query} />
  </div>
  {#if query.trim() === ""}
    <div class="min-h-0 flex-1"><WorkspaceBrowser /></div>
  {:else}
    <nav class="min-h-0 flex-1 overflow-auto p-2 text-sm" aria-label={t("workspace.search")}>
      {#if failure}<p class="p-2 text-destructive" role="alert">{failure}</p>{/if}
      {#each found as [page, lines] (page)}
        <button
          type="button"
          aria-current={workspace.page === page ? "page" : undefined}
          class="grid w-full gap-0.5 rounded-md p-2 text-start outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring aria-[current]:bg-accent"
          onclick={() => workspace.attempt(() => workspace.open(page))}
        >
          <span class="flex min-w-0 items-center gap-1.5 font-medium"><FileText class="size-4 shrink-0 text-muted-foreground" /><span class="truncate">{page}</span></span>
          {#each lines.filter((hit) => hit.line > 0).slice(0, 3) as hit (hit.line)}
            <span class="truncate ps-5.5 text-xs text-muted-foreground"><span class="tabular-nums">{hit.line}</span> · {hit.text}</span>
          {/each}
        </button>
      {:else}
        <p class="p-2 text-muted-foreground">{t("workspace.noHits")}</p>
      {/each}
    </nav>
  {/if}
</div>
