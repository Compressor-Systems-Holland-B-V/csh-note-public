<script lang="ts">
  import { minimalSetup } from "codemirror";
  import { Prec } from "@codemirror/state";
  import { EditorView, keymap, placeholder } from "@codemirror/view";
  import Book from "@lucide/svelte/icons/book";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Send from "@lucide/svelte/icons/send";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import * as NativeSelect from "$lib/components/ui/native-select";
  import { agent, efforts, models } from "$lib/state/agent.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { workspace } from "$lib/state/workspace.svelte";

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let empty = $state(true);

  const look = EditorView.theme({
    "&": { color: "var(--foreground)", backgroundColor: "transparent" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "inherit", lineHeight: "1.5" },
    ".cm-content": { minHeight: "3rem", padding: "0.5rem 0.625rem", caretColor: "var(--foreground)" },
    ".cm-line": { padding: "0" },
    ".cm-cursor": { borderLeftColor: "var(--foreground)" },
    ".cm-placeholder": { color: "var(--muted-foreground)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "var(--accent)",
    },
  });

  function send() {
    const text = view?.state.doc.toString() ?? "";
    if (!view || agent.running || text.trim() === "") return false;
    agent.send(text);
    view.dispatch({ changes: { from: 0, to: view.state.doc.length } });
    return true;
  }

  $effect(() => {
    const created = new EditorView({
      parent: host,
      extensions: [
        Prec.highest(keymap.of([{ key: "Mod-Enter", run: () => send() || true }])),
        minimalSetup,
        EditorView.lineWrapping,
        placeholder("Ask the assistant… Ctrl+Enter to send"),
        look,
        EditorView.contentAttributes.of({ "aria-label": "Message to the assistant" }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) empty = update.state.doc.toString().trim() === "";
        }),
      ],
    });
    view = created;
    return () => created.destroy();
  });
</script>

<form
  class="grid shrink-0 grid-cols-[minmax(0,1fr)] gap-2 border-t px-4 py-3"
  onsubmit={(event) => {
    event.preventDefault();
    send();
  }}
>
  {#if agent.references.length > 0}
    <ul class="flex flex-wrap gap-1" aria-label="What the assistant will look at">
      {#each agent.references as { label }, index (label)}
        <li class="flex items-center gap-1 rounded-md bg-accent py-0.5 pr-0.5 pl-1.5 text-xs">
          <span class="max-w-40 truncate" title={label}>{label}</span>
          <Button variant="ghost" size="icon-xs" aria-label="Remove {label}" onclick={() => agent.references.splice(index, 1)}>
            <X />
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
  <div
    bind:this={host}
    class="max-h-40 overflow-auto rounded-md border border-input shadow-xs focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30"
  ></div>
  <div class="flex flex-wrap items-center gap-1">
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label="Add a page or a file" title="Point the assistant to a page, or attach a file">
            <Paperclip />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start" class="w-72">
        <DropdownMenu.Item onSelect={() => agent.attach()}><Paperclip /> Attach files from this computer…</DropdownMenu.Item>
        {#if workspace.tree.length > 0}<DropdownMenu.Separator />{/if}
        {#each workspace.tree as project (project.name)}
          <DropdownMenu.Sub>
            <DropdownMenu.SubTrigger><Folder /> <span class="min-w-0 flex-1 truncate">{project.name}</span></DropdownMenu.SubTrigger>
            <DropdownMenu.SubContent class="w-72">
              <DropdownMenu.Item onSelect={() => agent.refer(project.name)}>The whole project</DropdownMenu.Item>
              {#each project.notebooks as notebook (notebook.name)}
                {@const path = `${project.name}/${notebook.name}`}
                <DropdownMenu.Sub>
                  <DropdownMenu.SubTrigger><Book /> <span class="min-w-0 flex-1 truncate">{notebook.name}</span></DropdownMenu.SubTrigger>
                  <DropdownMenu.SubContent class="w-72">
                    <DropdownMenu.Item onSelect={() => agent.refer(path)}>The whole notebook</DropdownMenu.Item>
                    {#each notebook.pages as page (page)}
                      <DropdownMenu.Item onSelect={() => agent.refer(`${path}/${page}`)}><FileText /> <span class="min-w-0 flex-1 truncate">{page}</span></DropdownMenu.Item>
                    {/each}
                  </DropdownMenu.SubContent>
                </DropdownMenu.Sub>
              {/each}
            </DropdownMenu.SubContent>
          </DropdownMenu.Sub>
        {/each}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
    <NativeSelect.Root
      size="sm"
      aria-label="Model"
      title="Model"
      value={settings.values.assistantModel ?? ""}
      onchange={(event) => settings.set("assistantModel", event.currentTarget.value || null)}
    >
      <NativeSelect.Option value="">Default model</NativeSelect.Option>
      {#each models as { id, label } (id)}
        <NativeSelect.Option value={id}>{label}</NativeSelect.Option>
      {/each}
    </NativeSelect.Root>
    <NativeSelect.Root
      size="sm"
      aria-label="How hard the assistant thinks"
      title="How hard the assistant thinks"
      value={settings.values.assistantEffort ?? ""}
      onchange={(event) => settings.set("assistantEffort", event.currentTarget.value || null)}
    >
      <NativeSelect.Option value="">Default thinking</NativeSelect.Option>
      {#each efforts as { id, label } (id)}
        <NativeSelect.Option value={id}>{label}</NativeSelect.Option>
      {/each}
    </NativeSelect.Root>
    <span class="flex-1"></span>
    {#if agent.here}
      <Button variant="destructive" size="sm" onclick={() => agent.stop()}><Square /> Stop</Button>
    {:else}
      <Button type="submit" size="sm" disabled={empty || agent.running} title={agent.running ? "The assistant is busy in another chat" : "Send (Ctrl+Enter)"}>
        <Send /> Send
      </Button>
    {/if}
  </div>
</form>
