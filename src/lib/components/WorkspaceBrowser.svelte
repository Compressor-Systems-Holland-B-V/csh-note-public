<script lang="ts">
  import Book from "@lucide/svelte/icons/book";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import Plus from "@lucide/svelte/icons/plus";
  import type { Component } from "svelte";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import { Button } from "@zsuite/ui/button";
  import * as ContextMenu from "@zsuite/ui/context-menu";
  import { askNew, askNewPage, askRemove, askRename, importPage } from "$lib/entries";
  import { agent } from "$lib/state/agent.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  let ask = $state<Ask | null>(null);
  let picked = $state<string | null>(null);

  const levels = ["own", "light", "medium", "strict"];
  const project = $derived(workspace.project);
  const notebooks = $derived(workspace.notebooks);
  const opened = $derived(workspace.page?.split("/") ?? []);
  const notebook = $derived(
    notebooks.find(({ name }) => name === picked) ?? notebooks.find(({ name }) => opened[0] === project && name === opened[1]) ?? notebooks[0],
  );
  const notebookPath = $derived(notebook ? `${project}/${notebook.name}` : null);

  const row =
    "flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-start outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring aria-[current]:bg-accent aria-[current]:font-medium";
</script>

{#snippet header(Icon: Component, label: string, add: (() => void) | null, addLabel: string)}
  <div class="flex h-8 shrink-0 items-center gap-1.5 border-b px-2 text-xs font-medium text-muted-foreground">
    <Icon class="size-3.5" />
    <span class="min-w-0 flex-1 truncate">{label}</span>
    {#if add}
      <Button variant="ghost" size="icon-xs" aria-label={addLabel} title={addLabel} onclick={add}><Plus /></Button>
    {/if}
  </div>
{/snippet}

{#snippet empty(text: string)}
  <p class="p-2 text-xs text-muted-foreground">{text}</p>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-sm">
  <nav class="flex h-8 shrink-0 items-center gap-1 overflow-hidden border-b px-3 text-xs text-muted-foreground" aria-label={t("workspace.browser.path")}>
    {#each [project, notebook?.name, workspace.page && opened[1] === notebook?.name ? opened[2] : null].filter(Boolean) as part, index (index)}
      {#if index > 0}<ChevronRight class="size-3 shrink-0 rtl:rotate-180" />{/if}
      <span class="truncate {index === 2 ? 'font-medium text-foreground' : ''}">{part}</span>
    {:else}
      {t("workspace.noProject")}
    {/each}
  </nav>
  <div class="grid min-h-0 flex-1 grid-cols-3 divide-x rtl:divide-x-reverse">
    <section class="flex min-h-0 flex-col" aria-label={t("workspace.browser.projects")}>
      {@render header(Folder, t("workspace.browser.projects"), () => (ask = askNew("project", "")), t("workspace.new.project"))}
      <ul class="min-h-0 flex-1 overflow-y-auto p-1">
        {#each workspace.tree as { name } (name)}
          <li>
            <ContextMenu.Root>
              <ContextMenu.Trigger>
                {#snippet child({ props })}
                  <button {...props} type="button" class={row} aria-current={project === name ? "true" : undefined} onclick={() => (workspace.project = name)}>
                    <Folder class="size-4 shrink-0 text-muted-foreground" /><span class="min-w-0 flex-1 truncate">{name}</span><ChevronRight class="size-3.5 shrink-0 text-muted-foreground rtl:rotate-180" />
                  </button>
                {/snippet}
              </ContextMenu.Trigger>
              <ContextMenu.Content>
                <ContextMenu.Item onSelect={() => (ask = askNew("notebook", name))}>{t("workspace.new.notebook")}</ContextMenu.Item>
                <ContextMenu.Item onSelect={() => (ask = askRename("project", name))}>{t("workspace.menu.rename")}</ContextMenu.Item>
                {#if privacy.options.knowledge}<ContextMenu.Item onSelect={() => privacy.add(name)}>{t("privacy.addToKnowledge")}</ContextMenu.Item>{/if}
                <ContextMenu.Item variant="destructive" onSelect={() => (ask = askRemove("project", name))}>{t("workspace.menu.delete")}</ContextMenu.Item>
              </ContextMenu.Content>
            </ContextMenu.Root>
          </li>
        {:else}
          {@render empty(t("workspace.noProjects"))}
        {/each}
      </ul>
    </section>

    <section class="flex min-h-0 flex-col" aria-label={t("workspace.browser.notebooks")}>
      {@render header(Book, t("workspace.browser.notebooks"), project ? () => (ask = askNew("notebook", project)) : null, t("workspace.new.notebook"))}
      <ul class="min-h-0 flex-1 overflow-y-auto p-1">
        {#each notebooks as { name, pages } (name)}
          {@const path = `${project}/${name}`}
          <li>
            <ContextMenu.Root>
              <ContextMenu.Trigger>
                {#snippet child({ props })}
                  <button {...props} type="button" class={row} aria-current={notebook?.name === name ? "true" : undefined} onclick={() => (picked = name)}>
                    <Book class="size-4 shrink-0 text-muted-foreground" /><span class="min-w-0 flex-1 truncate">{name}</span>
                    <span class="shrink-0 text-xs font-normal text-muted-foreground tabular-nums">{pages.length}</span>
                  </button>
                {/snippet}
              </ContextMenu.Trigger>
              <ContextMenu.Content>
                <ContextMenu.Item onSelect={() => (ask = askNewPage(path))}>{t("workspace.menu.newPage")}</ContextMenu.Item>
                <ContextMenu.Item onSelect={() => importPage(path)}>{t("workspace.menu.importPage")}</ContextMenu.Item>
                <ContextMenu.Item onSelect={() => (ask = askRename("notebook", path))}>{t("workspace.menu.rename")}</ContextMenu.Item>
                {#if privacy.options.knowledge}<ContextMenu.Item onSelect={() => privacy.add(path)}>{t("privacy.addToKnowledge")}</ContextMenu.Item>{/if}
                <ContextMenu.Item variant="destructive" onSelect={() => (ask = askRemove("notebook", path))}>{t("workspace.menu.delete")}</ContextMenu.Item>
              </ContextMenu.Content>
            </ContextMenu.Root>
          </li>
        {:else}
          {#if project}{@render empty(t("workspace.noNotebooks"))}{/if}
        {/each}
      </ul>
    </section>

    <section class="flex min-h-0 flex-col" aria-label={t("workspace.pages")}>
      {@render header(FileText, t("workspace.pages"), notebookPath ? () => (ask = askNewPage(notebookPath)) : null, t("workspace.new.page"))}
      <ul class="min-h-0 flex-1 overflow-y-auto p-1">
        {#each notebook?.pages ?? [] as page (page)}
          {@const path = `${notebookPath}/${page}`}
          <li>
            <ContextMenu.Root>
              <ContextMenu.Trigger>
                {#snippet child({ props })}
                  <button {...props} type="button" class={row} aria-current={workspace.page === path ? "page" : undefined} onclick={() => workspace.attempt(() => workspace.open(path))}>
                    <FileText class="size-4 shrink-0 text-muted-foreground" /><span class="min-w-0 flex-1 truncate">{page}</span>
                    {#if path in agent.pending}<span class="size-2 shrink-0 rounded-full bg-chart-2" role="img" aria-label={t("workspace.assistantChanged")} title={t("workspace.assistantChanged")}></span>{/if}
                    {#if privacy.hiding}<span class="shrink-0 text-xs font-normal text-muted-foreground">{t(`privacy.level.short.${levels[privacy.pageLevel(path)]}`)}</span>{/if}
                  </button>
                {/snippet}
              </ContextMenu.Trigger>
              <ContextMenu.Content>
                <ContextMenu.Item onSelect={() => (ask = askRename("page", path))}>{t("workspace.menu.rename")}</ContextMenu.Item>
                {#if privacy.options.knowledge}<ContextMenu.Item onSelect={() => privacy.add(path)}>{t("privacy.addToKnowledge")}</ContextMenu.Item>{/if}
                {#each notebooks as other (other.name)}
                  {#if other.name !== notebook?.name}
                    <ContextMenu.Item onSelect={() => workspace.attempt(() => workspace.move(path, `${project}/${other.name}/${page}`))}>{t("workspace.menu.moveTo", { notebook: other.name })}</ContextMenu.Item>
                  {/if}
                {/each}
                <ContextMenu.Item variant="destructive" onSelect={() => (ask = askRemove("page", path))}>{t("workspace.menu.delete")}</ContextMenu.Item>
              </ContextMenu.Content>
            </ContextMenu.Root>
          </li>
        {/each}
      </ul>
    </section>
  </div>
</div>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
