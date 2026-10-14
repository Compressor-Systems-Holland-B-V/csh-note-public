<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import FileBox from "@lucide/svelte/icons/file-box";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { appVersion } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { projects, titleOf } from "$lib/state/project.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { theme } from "$lib/state/theme.svelte";
  import Picker from "./Picker.svelte";
  import { languageOptions, t } from "$lib/i18n.svelte";

  const first = $derived(projects.file === null);
  const stay = $derived(first ? ("ignore" as const) : ("close" as const));
  const blocked = $derived(projects.busy || agent.running);
  const dark = $derived(settings.values.theme === "dark");
  const themeLabel = $derived(t(dark ? "common.appearance.toLight" : "common.appearance.toDark"));
</script>

{#snippet action(Icon: typeof FilePlus, label: string, hint: string | null, run: () => void)}
  <button
    type="button"
    class="group flex items-center gap-3 rounded-lg border bg-card p-3 text-start outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
    disabled={blocked}
    onclick={run}
  >
    <span class="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground group-hover:text-foreground"><Icon class="size-4" /></span>
    <span class="grid min-w-0">
      <span class="text-sm font-medium">{label}</span>
      {#if hint}<span class="truncate text-xs text-muted-foreground" title={hint}>{hint}</span>{/if}
    </span>
  </button>
{/snippet}

<Dialog.Root open onOpenChange={(open) => (projects.choosing = open || first)}>
  <Dialog.Content
    showCloseButton={false}
    escapeKeydownBehavior={stay}
    interactOutsideBehavior={stay}
    class="inset-0 flex h-full w-full max-w-none translate-x-0 translate-y-0 flex-col gap-0 rounded-none bg-background p-0 ring-0 sm:max-w-none"
  >
    <div class="flex h-10 shrink-0 items-center px-3">
      {#if !first}
        <Button variant="ghost" size="sm" onclick={() => (projects.choosing = false)}>
          <ArrowLeft class="rtl:rotate-180" /> {t("workspace.start.back", { name: projects.title ?? "" })}
        </Button>
      {/if}
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div class="mx-auto grid max-w-4xl gap-12 px-8 py-[8vh] md:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <div class="grid content-start gap-8">
          <div class="grid gap-3">
            {#if theme.logo}
              <img src={theme.logo} alt={theme.active?.name} class="h-8 justify-self-start" />
            {:else}
              <p class="text-4xl font-semibold tracking-tight">znote</p>
            {/if}
            <Dialog.Title class="text-xl font-normal">{t(first ? "workspace.file.welcome" : "workspace.file.title")}</Dialog.Title>
            <Dialog.Description class="text-sm text-muted-foreground">{t("workspace.file.text")}</Dialog.Description>
          </div>

          <section class="grid gap-2" aria-labelledby="start-actions">
            <h2 id="start-actions" class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              {t("workspace.start.start")}
              {#if projects.busy}<LoaderCircle class="size-3.5 animate-spin" />{/if}
            </h2>
            {@render action(FilePlus, t("workspace.file.new"), null, () => projects.create())}
            {@render action(FolderOpen, t("workspace.file.open"), null, () => projects.open())}
            {#if projects.oldFolder}
              {@const folder = projects.oldFolder}
              {@render action(FolderInput, t("workspace.file.bringIn"), folder, () => projects.create(folder))}
            {/if}
          </section>
          {#if projects.failure}<p class="text-sm wrap-anywhere whitespace-pre-line text-destructive" role="alert">{projects.failure}</p>{/if}
        </div>

        <section class="grid content-start gap-2" aria-labelledby="start-recent">
          <h2 id="start-recent" class="text-xs font-medium text-muted-foreground">{t("workspace.file.recent")}</h2>
          {#if projects.recent.length === 0}
            <p class="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">{t("workspace.start.empty")}</p>
          {:else}
            <ul class="grid gap-0.5">
              {#each projects.recent as file (file)}
                <li class="group flex min-w-0 items-center gap-1 rounded-md hover:bg-accent">
                  <button
                    type="button"
                    class="flex min-w-0 flex-1 items-center gap-3 rounded-md px-2 py-2 text-start outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                    disabled={blocked}
                    onclick={() => projects.open(file)}
                  >
                    <FileBox class="size-4 shrink-0 text-muted-foreground" />
                    <span class="grid min-w-0">
                      <span class="truncate text-sm font-medium">{titleOf(file)}</span>
                      <span class="truncate text-xs text-muted-foreground" title={file}>{file}</span>
                    </span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    class="me-1 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                    aria-label={t("workspace.file.forgetFile", { name: titleOf(file) })}
                    title={t("workspace.file.forget")}
                    onclick={() => projects.forget(file)}
                  >
                    <X />
                  </Button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      </div>
    </div>

    <footer class="flex h-10 shrink-0 items-center justify-between gap-2 border-t px-3 text-xs text-muted-foreground">
      <span>{#await appVersion() then version}{t("common.version", { version })}{:catch}{/await}</span>
      <div class="flex items-center gap-1">
        <Picker size="sm" aria-label={t("settings.language")} value={settings.values.language} options={languageOptions()} onchange={(language) => settings.set("language", language)} />
        <Button variant="ghost" size="icon-sm" aria-label={themeLabel} title={themeLabel} onclick={() => settings.set("theme", dark ? "light" : "dark")}>
          {#if dark}<Sun />{:else}<Moon />{/if}
        </Button>
      </div>
    </footer>
  </Dialog.Content>
</Dialog.Root>
