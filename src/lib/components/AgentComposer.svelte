<script lang="ts">
  import { minimalSetup } from "codemirror";
  import { Prec } from "@codemirror/state";
  import { EditorView, keymap, placeholder } from "@codemirror/view";
  import Book from "@lucide/svelte/icons/book";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import Globe from "@lucide/svelte/icons/globe";
  import GlobeOff from "@lucide/svelte/icons/globe-off";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import Send from "@lucide/svelte/icons/send";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { presetHint, presetName, presetsFor, standard } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import PresetDialog from "./PresetDialog.svelte";
  import { t } from "$lib/i18n.svelte";

  let host: HTMLDivElement;
  let advanced = $state(false);

  const provider = $derived(agent.using ?? "claude");
  const presets = $derived(presetsFor(provider, settings.values.assistantPresets[provider] ?? []));
  const sized = $derived(presets.filter(({ id }) => standard(id)));
  const own = $derived(presets.filter(({ id }) => !standard(id)));
  const custom = $derived(!standard(agent.preset.id));

  function useOwn() {
    if (own.length === 0) advanced = true;
    else if (!custom) settings.set("assistantPreset", own[own.length - 1].id);
  }
  let view: EditorView | undefined;
  let empty = $state(true);

  const look = EditorView.theme({
    "&": { height: "100%", color: "var(--foreground)", backgroundColor: "transparent" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { overflow: "auto", fontFamily: "inherit", lineHeight: "1.5" },
    ".cm-content": { padding: "0.5rem 0.625rem", caretColor: "var(--foreground)" },
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
        placeholder(t("assistant.composer.placeholder")),
        look,
        EditorView.contentAttributes.of({ "aria-label": t("assistant.composer.message") }),
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
    <ul class="flex flex-wrap gap-1" aria-label={t("assistant.composer.references")}>
      {#each agent.references as { label }, index (label)}
        <li class="flex items-center gap-1 rounded-md bg-accent py-0.5 pe-0.5 ps-1.5 text-xs">
          <span class="max-w-40 truncate" title={label}>{label}</span>
          <Button variant="ghost" size="icon-xs" aria-label={t("assistant.composer.removeReference", { label })} onclick={() => agent.references.splice(index, 1)}>
            <X />
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
  <div
    bind:this={host}
    class="h-40 overflow-hidden rounded-md border border-input shadow-xs focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30"
  ></div>
  <div class="flex gap-0.5 rounded-md border p-0.5" role="radiogroup" aria-label={t("assistant.composer.presets")}>
    {#each sized as { id, name, hint } (id)}
      <Button
        variant={agent.preset.id === id ? "secondary" : "ghost"}
        size="xs"
        class="min-w-0 flex-auto px-1.5"
        role="radio"
        aria-checked={agent.preset.id === id}
        title={presetHint({ id, hint })}
        onclick={() => settings.set("assistantPreset", id)}
      >
        <span class="truncate">{presetName({ id, name })}</span>
      </Button>
    {/each}
    <Button
      variant={custom ? "secondary" : "ghost"}
      size="xs"
      class="min-w-0 flex-auto px-1.5"
      role="radio"
      aria-checked={custom}
      title={custom ? agent.preset.name : t("assistant.composer.advancedTitle")}
      onclick={useOwn}
    >
      <span class="truncate">{t("assistant.size.custom")}</span>
    </Button>
  </div>
  <div class="flex items-center gap-1">
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label={t("assistant.composer.add")} title={t("assistant.composer.addTitle")}>
            <Paperclip />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start" class="w-72">
        <DropdownMenu.Item onSelect={() => agent.attach()}><Paperclip /> {t("assistant.composer.attach")}</DropdownMenu.Item>
        {#if workspace.tree.length > 0}<DropdownMenu.Separator />{/if}
        {#each workspace.tree as project (project.name)}
          <DropdownMenu.Sub>
            <DropdownMenu.SubTrigger><Folder /> <span class="min-w-0 flex-1 truncate">{project.name}</span></DropdownMenu.SubTrigger>
            <DropdownMenu.SubContent class="w-72">
              <DropdownMenu.Item onSelect={() => agent.refer(project.name)}>{t("assistant.composer.wholeProject")}</DropdownMenu.Item>
              {#each project.notebooks as notebook (notebook.name)}
                {@const path = `${project.name}/${notebook.name}`}
                <DropdownMenu.Sub>
                  <DropdownMenu.SubTrigger><Book /> <span class="min-w-0 flex-1 truncate">{notebook.name}</span></DropdownMenu.SubTrigger>
                  <DropdownMenu.SubContent class="w-72">
                    <DropdownMenu.Item onSelect={() => agent.refer(path)}>{t("assistant.composer.wholeNotebook")}</DropdownMenu.Item>
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
    {#if agent.preset.web === null}
      <Button variant={agent.web ? "secondary" : "ghost"} size="icon-sm" aria-pressed={agent.web} aria-label={t("assistant.composer.web")} title={t("assistant.composer.web")} onclick={() => (agent.web = !agent.web)}>
        {#if agent.web}<Globe />{:else}<GlobeOff />{/if}
      </Button>
    {/if}
    <Button variant="ghost" size="icon-sm" aria-label={t("assistant.composer.advanced")} title={t("assistant.composer.advancedTitle")} onclick={() => (advanced = true)}>
      <SlidersHorizontal />
    </Button>
    {#if !custom}<ModelPicker class="w-auto max-w-48 min-w-0 shrink" />{/if}
    <span class="flex-1"></span>
    {#if agent.here}
      <Button variant="destructive" size="sm" class="shrink-0" onclick={() => agent.stop()}><Square /> {t("assistant.composer.stop")}</Button>
    {:else}
      <Button type="submit" size="sm" class="shrink-0" disabled={empty || agent.running} title={t(agent.running ? "assistant.composer.busyElsewhere" : "assistant.composer.sendTitle")}>
        <Send /> {t("assistant.composer.send")}
      </Button>
    {/if}
  </div>
</form>

<PresetDialog bind:open={advanced} />
