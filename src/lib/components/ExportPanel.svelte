<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { Button } from "$lib/components/ui/button";
  import { exportPage, openExport, showExport } from "$lib/ipc";
  import type { Exported, ExportFormat } from "$lib/ipc";
  import { doc } from "$lib/state/document.svelte";
  import { templateKind, theme } from "$lib/state/theme.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { messageOf } from "$lib/utils";
  import { t } from "$lib/i18n.svelte";

  const formats: { id: ExportFormat; label: string }[] = [
    { id: "pdf", label: "PDF" },
    { id: "pptx", label: "PowerPoint" },
  ];

  let chosen = $state<ExportFormat | null>(null);
  let busy = $state(false);
  let done = $state<Exported | null>(null);
  let failure = $state<string | null>(null);

  const suggested = $derived(templateKind(doc.text, theme.templates) === "slides" ? "pptx" : "pdf");
  const format = $derived(chosen ?? suggested);
  const blocked = $derived.by(() => {
    if (workspace.page === null) return t("export.blocked.noPage");
    if (doc.diagnostics.some((diagnostic) => diagnostic.severity === "error"))
      return t("export.blocked.problems");
    if (doc.pages.length === 0) return t("export.blocked.empty");
    return null;
  });

  async function attempt(action: () => Promise<void>) {
    failure = null;
    try {
      await action();
    } catch (error) {
      failure = messageOf(error);
    }
  }

  async function run() {
    const page = workspace.page;
    if (page === null) return;
    busy = true;
    done = null;
    await attempt(async () => {
      done = await exportPage(page, doc.text, format);
    });
    busy = false;
  }
</script>

<aside class="flex w-72 shrink-0 flex-col gap-3 overflow-auto border-s bg-sidebar p-3 text-sm" aria-labelledby="export-title">
  <h2 id="export-title" class="font-medium">{t("export.title")}</h2>

  <div class="grid gap-1" role="group" aria-label={t("export.format")}>
    {#each formats as { id, label } (id)}
      <Button
        variant={format === id ? "secondary" : "ghost"}
        class="h-auto flex-col items-start gap-0.5 py-2 text-start whitespace-normal"
        aria-pressed={format === id}
        onclick={() => (chosen = id)}
      >
        <span>{label}</span>
        <span class="text-xs font-normal text-muted-foreground">{t(`export.${id}Hint`)}</span>
      </Button>
    {/each}
  </div>

  {#if blocked}
    <p class="text-muted-foreground" role="status">{blocked}</p>
  {:else}
    <Button disabled={busy} onclick={run}>{t(busy ? "export.running" : "export.run")}</Button>
  {/if}

  {#if failure}<p class="text-destructive" role="alert">{failure}</p>{/if}

  {#if done}
    <div class="grid gap-2 border-t pt-3" role="status">
      <p class="break-all">{t("export.savedTo", { path: done.path })}</p>
      {#if done.missingFonts.length > 0}
        <p class="text-muted-foreground">{t("export.missingFonts", { fonts: done.missingFonts.join(", ") })}</p>
      {/if}
      <div class="flex gap-1">
        <Button variant="outline" size="sm" onclick={() => attempt(openExport)}>{t("export.openFile")}</Button>
        <Button variant="outline" size="sm" onclick={() => attempt(showExport)}>{t("export.showInFolder")}</Button>
      </div>
    </div>
  {/if}
</aside>

{#if busy}
  <div class="fixed inset-0 z-50 flex flex-col items-center justify-center gap-3 bg-background/80 text-sm" role="status">
    <LoaderCircle class="size-8 animate-spin" />
    {t("export.runningPage")}
  </div>
{/if}
