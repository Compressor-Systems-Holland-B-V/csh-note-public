<script lang="ts">
  import Book from "@lucide/svelte/icons/book";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import Plus from "@lucide/svelte/icons/plus";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import { Button } from "$lib/components/ui/button";
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { theme, withTemplate } from "$lib/state/theme.svelte";
  import { pickTypstFile } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { childOf, nameOf, parentOf, workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  let ask = $state<Ask | null>(null);

  const project = $derived(workspace.project);
  const row = "rounded-md outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring";

  function add(kind: string, parent: string) {
    ask = {
      title: t(`workspace.new.${kind}`),
      value: "",
      action: t("common.create"),
      run: (name) => workspace.create(childOf(parent, name)),
    };
  }

  function addPage(notebook: string) {
    const templates = theme.offered;
    ask = {
      title: t("workspace.new.page"),
      value: "",
      action: t("common.create"),
      options: [t("workspace.noTemplate"), ...templates.map((template) => template.label)],
      run: async (name, option) => {
        await workspace.create(childOf(notebook, name));
        if (option > 0) workspace.rewrite((text) => withTemplate(text, templates[option - 1]));
      },
    };
  }

  function importPage(notebook: string) {
    return workspace.attempt(async () => {
      const file = await pickTypstFile();
      if (file !== null) await workspace.importPage(notebook, file);
    });
  }

  function rename(kind: string, path: string) {
    ask = {
      title: t(`workspace.rename.${kind}`),
      value: nameOf(path),
      action: t("common.rename"),
      run: (name) => workspace.move(path, childOf(parentOf(path), name)),
    };
  }

  function remove(kind: string, path: string) {
    ask = {
      title: t(`workspace.remove.${kind}`, { name: nameOf(path) }),
      text: t("workspace.remove.text"),
      action: t("common.delete"),
      destructive: true,
      run: () => workspace.remove(path),
    };
  }
</script>

<div class="flex h-full flex-col text-sm">
  <div class="flex shrink-0 border-b p-1">
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="sm" class="min-w-0 flex-1 justify-between" title={t("workspace.switchProject")}>
            <Folder />
            <span class="min-w-0 flex-1 truncate text-start">{project ?? t("workspace.noProject")}</span>
            <ChevronsUpDown />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start">
        {#each workspace.tree as { name } (name)}
          <DropdownMenu.Item onSelect={() => (workspace.project = name)}><Folder /> {name}</DropdownMenu.Item>
        {/each}
        {#if workspace.tree.length > 0}<DropdownMenu.Separator />{/if}
        <DropdownMenu.Item onSelect={() => add("project", "")}>{t("workspace.menu.newProject")}</DropdownMenu.Item>
        {#if project}
          <DropdownMenu.Item onSelect={() => rename("project", project)}>{t("workspace.menu.renameProject")}</DropdownMenu.Item>
          {#if privacy.options.knowledge}
            <DropdownMenu.Item onSelect={() => privacy.add(project)}>{t("privacy.addToKnowledge")}</DropdownMenu.Item>
          {/if}
          <DropdownMenu.Item variant="destructive" onSelect={() => remove("project", project)}>
            {t("workspace.menu.deleteProject")}
          </DropdownMenu.Item>
        {/if}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>

  <nav class="min-h-0 flex-1 overflow-auto p-1" aria-label={t("workspace.pages")}>
    {#each workspace.notebooks as notebook (notebook.name)}
      {@const path = `${project}/${notebook.name}`}
      <details open class="group">
        <ContextMenu.Root>
          <ContextMenu.Trigger>
            {#snippet child({ props })}
              <summary {...props} class="flex cursor-default items-center gap-1 p-1 font-medium {row}">
                <ChevronRight class="size-4 shrink-0 transition-transform group-open:rotate-90 rtl:rotate-180 rtl:group-open:rotate-90" />
                <Book class="size-4 shrink-0 text-muted-foreground" />
                <span class="min-w-0 flex-1 truncate">{notebook.name}</span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={t("workspace.new.pageIn", { notebook: notebook.name })}
                  title={t("workspace.new.page")}
                  onclick={(event) => {
                    event.preventDefault();
                    addPage(path);
                  }}
                >
                  <Plus />
                </Button>
              </summary>
            {/snippet}
          </ContextMenu.Trigger>
          <ContextMenu.Content>
            <ContextMenu.Item onSelect={() => addPage(path)}>{t("workspace.menu.newPage")}</ContextMenu.Item>
            <ContextMenu.Item onSelect={() => importPage(path)}>{t("workspace.menu.importPage")}</ContextMenu.Item>
            <ContextMenu.Item onSelect={() => rename("notebook", path)}>{t("workspace.menu.rename")}</ContextMenu.Item>
            {#if privacy.options.knowledge}
              <ContextMenu.Item onSelect={() => privacy.add(path)}>{t("privacy.addToKnowledge")}</ContextMenu.Item>
            {/if}
            <ContextMenu.Item variant="destructive" onSelect={() => remove("notebook", path)}>{t("workspace.menu.delete")}</ContextMenu.Item>
          </ContextMenu.Content>
        </ContextMenu.Root>
        {#each notebook.pages as page (page)}
          {@const pagePath = `${path}/${page}`}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              {#snippet child({ props })}
                <button
                  {...props}
                  type="button"
                  aria-current={workspace.page === pagePath ? "page" : undefined}
                  class="flex w-full items-center gap-1 py-1 pe-2 ps-6 text-start aria-[current]:bg-accent aria-[current]:font-medium {row}"
                  onclick={() => workspace.attempt(() => workspace.open(pagePath))}
                >
                  <FileText class="size-4 shrink-0 text-muted-foreground" />
                  <span class="min-w-0 flex-1 truncate">{page}</span>
                  {#if pagePath in agent.pending}
                    <span class="size-2 shrink-0 rounded-full bg-chart-2" role="img" aria-label={t("workspace.assistantChanged")} title={t("workspace.assistantChanged")}></span>
                  {/if}
                </button>
              {/snippet}
            </ContextMenu.Trigger>
            <ContextMenu.Content>
              <ContextMenu.Item onSelect={() => rename("page", pagePath)}>{t("workspace.menu.rename")}</ContextMenu.Item>
              {#if privacy.options.knowledge}
                <ContextMenu.Item onSelect={() => privacy.add(pagePath)}>{t("privacy.addToKnowledge")}</ContextMenu.Item>
              {/if}
              {#each workspace.notebooks as other (other.name)}
                {#if other.name !== notebook.name}
                  <ContextMenu.Item
                    onSelect={() => workspace.attempt(() => workspace.move(pagePath, `${project}/${other.name}/${page}`))}
                  >
                    {t("workspace.menu.moveTo", { notebook: other.name })}
                  </ContextMenu.Item>
                {/if}
              {/each}
              <ContextMenu.Item variant="destructive" onSelect={() => remove("page", pagePath)}>{t("workspace.menu.delete")}</ContextMenu.Item>
            </ContextMenu.Content>
          </ContextMenu.Root>
        {/each}
      </details>
    {:else}
      <p class="p-2 text-muted-foreground">
        {t(project ? "workspace.noNotebooks" : "workspace.noProjects")}
      </p>
    {/each}
  </nav>

  <div class="flex shrink-0 items-center border-t p-1">
    {#if project}
      <Button variant="ghost" size="sm" onclick={() => add("notebook", project)}><Plus /> {t("workspace.new.notebook")}</Button>
    {:else}
      <Button variant="ghost" size="sm" onclick={() => add("project", "")}><Plus /> {t("workspace.new.project")}</Button>
    {/if}
  </div>
</div>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
