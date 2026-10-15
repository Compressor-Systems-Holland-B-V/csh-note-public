<script lang="ts">
  import Eye from "@lucide/svelte/icons/eye";
  import { untrack } from "svelte";
  import * as Dialog from "@zsuite/ui/dialog";
  import { referenceKind } from "$lib/assistant";
  import type { Attachment, Seen } from "$lib/ipc";
  import { structured } from "$lib/markdown";
  import { agent } from "$lib/state/agent.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { pagesOf, workspace } from "$lib/state/workspace.svelte";
  import { messageOf } from "$lib/utils";
  import LevelSlider from "./LevelSlider.svelte";
  import PrivacyView from "./PrivacyView.svelte";
  import ReferenceIcon from "./ReferenceIcon.svelte";
  import { t } from "$lib/i18n.svelte";

  let { onclose }: { onclose: () => void } = $props();

  type Row = { key: string; label: string; level: number | undefined; markdown: boolean; load: () => Promise<Seen[]>; onlevel?: (level: number) => void };

  let files = $state<Attachment[]>([]);
  let picked = $state<string | null>(null);
  let shown = $state<Seen[]>([]);
  let failure = $state<string | null>(null);

  function pagesUnder(label: string) {
    const pages = referenceKind(label) === "page" ? [label] : pagesOf(workspace.tree).filter((page) => page.startsWith(`${label}/`));
    return Promise.all(pages.map(async (page) => ({ label: page, text: await workspace.read(page) })));
  }

  function setFileLevel(file: Attachment, level: number) {
    file.level = level;
    agent.setLevel(file.path, level);
  }

  const rows = $derived<Row[]>([
    ...agent.references
      .filter((reference) => reference.path === undefined)
      .map(({ label }) => {
        const page = referenceKind(label) === "page";
        return {
          key: label,
          label,
          level: page ? privacy.pageLevel(label) : undefined,
          markdown: false,
          load: () => pagesUnder(label),
          onlevel: page ? (level: number) => privacy.setPageLevel(label, level) : undefined,
        };
      }),
    ...files.map((file) => ({
      key: file.path,
      label: file.name,
      level: file.level,
      markdown: structured(file.path),
      load: async () => [{ label: file.name, text: file.text ?? "" }],
      onlevel: (level: number) => setFileLevel(file, level),
    })),
  ]);
  const chosen = $derived(rows.find((row) => row.key === picked) ?? rows[0]);
  const chosenKey = $derived(chosen?.key);

  $effect(() => {
    untrack(() => agent.files()).then(
      (found) => (files = found),
      (error: unknown) => (failure = messageOf(error)),
    );
  });

  $effect(() => {
    void chosenKey;
    const row = untrack(() => chosen);
    shown = [];
    row?.load().then(
      (seen) => {
        if (chosen?.key === row.key) shown = seen;
      },
      (error: unknown) => (failure = messageOf(error)),
    );
  });
</script>

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) onclose();
  }}
>
  <Dialog.Content class="flex h-[85vh] flex-col sm:max-w-5xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2"><Eye class="size-5 text-chart-2" /> {t("assistant.files.title")}</Dialog.Title>
      <Dialog.Description>{t("assistant.files.text")}</Dialog.Description>
    </Dialog.Header>
    {#if failure}<p class="text-sm text-destructive" role="alert">{failure}</p>{/if}
    {#if rows.length === 0}
      <p class="flex flex-1 items-center justify-center text-center text-sm text-muted-foreground">{t("assistant.files.none")}</p>
    {:else}
      <div class="grid min-h-0 flex-1 grid-cols-[16rem_minmax(0,1fr)] gap-4">
        <ul class="flex min-w-0 flex-col gap-0.5 overflow-y-auto" aria-label={t("assistant.files.title")}>
          {#each rows as row (row.key)}
            <li>
              <button
                type="button"
                aria-current={chosen?.key === row.key ? "true" : undefined}
                class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-start text-sm outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring aria-[current]:bg-accent"
                onclick={() => (picked = row.key)}
              >
                <ReferenceIcon label={row.label} />
                <span class="min-w-0 flex-1 truncate" title={row.label}>{row.label}</span>
              </button>
            </li>
          {/each}
        </ul>
        {#if chosen}
          <div class="flex min-h-0 flex-col gap-3">
            {#if chosen.onlevel && chosen.level !== undefined}<LevelSlider class="shrink-0" level={chosen.level} onlevel={chosen.onlevel} />{/if}
            <div class="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto">
              {#each shown as { label, text }, index (index)}
                <div class="grid gap-1">
                  {#if shown.length > 1}<p class="text-sm font-medium">{label}</p>{/if}
                  <div class="overflow-hidden rounded-md border"><PrivacyView {text} level={chosen.level} markdown={chosen.markdown} /></div>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>
