<script lang="ts">
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { agent } from "$lib/state/agent.svelte";
  import { projects, titleOf } from "$lib/state/project.svelte";
  import { t } from "$lib/i18n.svelte";

  const first = $derived(projects.file === null);
  const stay = $derived(first ? ("ignore" as const) : ("close" as const));
  const blocked = $derived(projects.busy || agent.running);
</script>

<Dialog.Root open onOpenChange={(open) => (projects.choosing = open || first)}>
  <Dialog.Content showCloseButton={!first} escapeKeydownBehavior={stay} interactOutsideBehavior={stay}>
    <Dialog.Header>
      <Dialog.Title>{t(first ? "workspace.file.welcome" : "workspace.file.title")}</Dialog.Title>
      <Dialog.Description>
        {t("workspace.file.text")}
      </Dialog.Description>
    </Dialog.Header>
    <div class="grid gap-2">
      <Button class="justify-start" disabled={blocked} onclick={() => projects.create()}>
        {#if projects.busy}<LoaderCircle class="animate-spin" />{:else}<FilePlus />{/if} {t("workspace.file.new")}
      </Button>
      <Button variant="outline" class="justify-start" disabled={blocked} onclick={() => projects.open()}>
        <FolderOpen /> {t("workspace.file.open")}
      </Button>
      {#if projects.oldFolder}
        {@const folder = projects.oldFolder}
        <Button variant="outline" class="h-auto justify-start py-2 text-start whitespace-normal" disabled={blocked} onclick={() => projects.create(folder)}>
          <FolderInput />
          <span class="grid min-w-0">
            <span>{t("workspace.file.bringIn")}</span>
            <span class="truncate text-xs font-normal text-muted-foreground" title={folder}>{folder}</span>
          </span>
        </Button>
      {/if}
    </div>
    {#if projects.recent.length > 0}
      <div class="grid gap-1">
        <p class="text-xs font-medium text-muted-foreground">{t("workspace.file.recent")}</p>
        <ul class="grid max-h-48 gap-0.5 overflow-y-auto">
          {#each projects.recent as file (file)}
            <li class="flex min-w-0 items-center gap-1 rounded-md hover:bg-accent">
              <button
                type="button"
                class="grid min-w-0 flex-1 rounded-md px-2 py-1.5 text-start text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                disabled={blocked}
                onclick={() => projects.open(file)}
              >
                <span class="truncate">{titleOf(file)}</span>
                <span class="truncate text-xs text-muted-foreground" title={file}>{file}</span>
              </button>
              <Button variant="ghost" size="icon-xs" aria-label={t("workspace.file.forgetFile", { name: titleOf(file) })} title={t("workspace.file.forget")} onclick={() => projects.forget(file)}>
                <X />
              </Button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {#if projects.failure}<p class="text-sm wrap-anywhere whitespace-pre-line text-destructive" role="alert">{projects.failure}</p>{/if}
  </Dialog.Content>
</Dialog.Root>
