<script lang="ts">
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FileBox from "@lucide/svelte/icons/file-box";
  import FileOutput from "@lucide/svelte/icons/file-output";
  import Moon from "@lucide/svelte/icons/moon";
  import Store from "@lucide/svelte/icons/store";
  import Pencil from "@lucide/svelte/icons/pencil";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Settings from "@lucide/svelte/icons/settings";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Sun from "@lucide/svelte/icons/sun";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import AgentPanel from "$lib/components/AgentPanel.svelte";
  import ExportPanel from "$lib/components/ExportPanel.svelte";
  import EditorColumn from "$lib/components/EditorColumn.svelte";
  import KnowledgeUnlock from "$lib/components/KnowledgeUnlock.svelte";
  import Preview from "$lib/components/Preview.svelte";
  import FirstStart from "$lib/components/FirstStart.svelte";
  import PageSearch from "$lib/components/PageSearch.svelte";
  import RedactionPanel from "$lib/components/RedactionPanel.svelte";
  import ProjectNotices from "$lib/components/ProjectNotices.svelte";
  import ProjectStart from "$lib/components/ProjectStart.svelte";
  import Picker from "$lib/components/Picker.svelte";
  import SettingsDialog from "$lib/components/SettingsDialog.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import SnapshotButtons from "$lib/components/SnapshotButtons.svelte";
  import ThemeGallery from "$lib/components/ThemeGallery.svelte";
  import { Button } from "$lib/components/ui/button";
  import * as Resizable from "$lib/components/ui/resizable";
  import * as Tabs from "$lib/components/ui/tabs";
  import { Separator } from "$lib/components/ui/separator";
  import { appVersion } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { chats } from "$lib/state/chats.svelte";
  import { doc } from "$lib/state/document.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { projects } from "$lib/state/project.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { theme } from "$lib/state/theme.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { i18n, languageOptions, t } from "$lib/i18n.svelte";

  let settingsOpen = $state(false);
  let settingsSection = $state("general");
  let assistantOpen = $state(false);
  let chosen = $state<"edit" | "export" | "themes">("edit");
  let agentTab = $state("preview");

  const layoutIcons = { edit: Pencil, agent: MessagesSquare, redaction: EyeOff };
  const layout = $derived(settings.values.layout);
  const modes = $derived([
    { id: "edit" as const, icon: layoutIcons[layout], label: t(`common.layout.${layout}`) },
    { id: "export" as const, icon: FileOutput, label: t("common.mode.export") },
    ...(settings.values.marketplace ? [{ id: "themes" as const, icon: Store, label: t("common.mode.themes") }] : []),
  ]);
  const mode = $derived(modes.some(({ id }) => id === chosen) ? chosen : "edit");

  function openSettings(section = "general") {
    settingsSection = section;
    settingsOpen = true;
  }

  const dark = $derived(settings.values.theme === "dark");
  const themeLabel = $derived(t(dark ? "common.appearance.toLight" : "common.appearance.toDark"));
  const assistantLabel = $derived(t(assistantOpen ? "common.assistant.hide" : "common.assistant.show"));
  const problems = $derived([workspace.failure, projects.choosing ? null : projects.failure, settings.failure, theme.failure, chats.failure].filter((failure) => failure !== null));

  function saveOnShortcut(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "s") return;
    event.preventDefault();
    workspace.save();
  }
</script>

{#snippet preview(startZoom: number | null)}
  {#if settings.loaded}
    <Preview pages={doc.pages} {startZoom} loading={doc.loading} onrefresh={() => doc.refresh()} />
  {/if}
{/snippet}

<svelte:window onkeydown={saveOnShortcut} />

<div class="flex h-screen flex-col">
  <header class="flex h-10 shrink-0 items-center justify-between border-b px-3 text-sm font-medium">
    <div class="flex min-w-0 items-center gap-3">
      {#if theme.logo}<img src={theme.logo} alt={theme.active?.name} class="h-5" />{:else}znote{/if}
      {#if projects.title}
        <Separator orientation="vertical" class="my-2.5" />
        <button
          type="button"
          class="flex min-w-0 items-center gap-1.5 rounded-md bg-muted px-2 py-0.5 font-normal outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          title={`${t("workspace.file.switch")}\n${projects.file}${projects.savedAt === null ? "" : `\n${t("workspace.file.savedAt", { time: new Date(projects.savedAt).toLocaleTimeString(i18n.locale) })}`}`}
          disabled={agent.running}
          onclick={() => (projects.choosing = true)}
        >
          <FileBox class="size-3.5 shrink-0 text-muted-foreground" />
          <span class="truncate">{projects.title}</span><span class="-ms-1.5 text-muted-foreground">.znote</span>
        </button>
      {/if}
    </div>
    <div class="flex items-center gap-1">
      {#each modes as { id, icon: Icon, label } (id)}
        <Button variant={mode === id ? "secondary" : "ghost"} size="sm" aria-pressed={mode === id} onclick={() => (chosen = id)}>
          <Icon /> {label}
        </Button>
      {/each}
    </div>
    <div class="flex items-center gap-1">
      <SnapshotButtons />
      <Picker size="sm" class="w-32" aria-label={t("settings.language")} title={t("settings.language")} value={settings.values.language} options={languageOptions()} onchange={(language) => settings.set("language", language)} />
      <Button variant="ghost" size="icon-sm" aria-label={t("common.settings")} title={t("common.settings")} onclick={() => openSettings()}>
        <Settings />
      </Button>
    </div>
  </header>

  {#if mode === "export"}
    <div class="flex min-h-0 flex-1">
      <div class="min-w-0 flex-1">
        {@render preview(1)}
      </div>
      {#key workspace.page}<ExportPanel />{/key}
    </div>
  {:else if mode === "themes"}
    <div class="min-h-0 flex-1"><ThemeGallery /></div>
  {:else if layout === "agent"}
    <Resizable.PaneGroup direction="horizontal" class="min-h-0 flex-1">
      <Resizable.Pane defaultSize={16} minSize={10} class="bg-sidebar">
        <Sidebar />
      </Resizable.Pane>
      <Resizable.Handle />
      <Resizable.Pane defaultSize={44} minSize={25}>
        <AgentPanel wide />
      </Resizable.Pane>
      <Resizable.Handle />
      <Resizable.Pane defaultSize={40} minSize={20}>
        <Tabs.Root bind:value={agentTab} class="h-full gap-0">
          <Tabs.List class="m-1 shrink-0">
            <Tabs.Trigger value="preview">{t("common.preview")}</Tabs.Trigger>
            <Tabs.Trigger value="editor">{t("settings.editor")}</Tabs.Trigger>
          </Tabs.List>
          <Tabs.Content value="preview" class="min-h-0 border-t">{@render preview(settings.values.zoom)}</Tabs.Content>
          <Tabs.Content value="editor" class="min-h-0 border-t"><EditorColumn /></Tabs.Content>
        </Tabs.Root>
      </Resizable.Pane>
    </Resizable.PaneGroup>
  {:else if layout === "redaction"}
    <Resizable.PaneGroup direction="horizontal" class="min-h-0 flex-1">
      <Resizable.Pane defaultSize={26} minSize={18} class="bg-sidebar">
        <AgentPanel />
      </Resizable.Pane>
      <Resizable.Handle />
      <Resizable.Pane defaultSize={38} minSize={20}>
        <PageSearch />
      </Resizable.Pane>
      <Resizable.Handle />
      <Resizable.Pane defaultSize={36} minSize={20}>
        <RedactionPanel onsettings={() => openSettings("privacy")} />
      </Resizable.Pane>
    </Resizable.PaneGroup>
  {:else}
    <Resizable.PaneGroup direction="horizontal" class="min-h-0 flex-1">
      <Resizable.Pane defaultSize={16} minSize={10} class="bg-sidebar">
        <Sidebar />
      </Resizable.Pane>
      <Resizable.Handle />
      <Resizable.Pane defaultSize={32} minSize={15}>
        <EditorColumn />
      </Resizable.Pane>
      <Resizable.Handle />
      <Resizable.Pane defaultSize={32} minSize={15}>
        {@render preview(settings.values.zoom)}
      </Resizable.Pane>
      {#if assistantOpen}
        <Resizable.Handle />
        <Resizable.Pane defaultSize={20} minSize={15} class="bg-sidebar">
          <AgentPanel />
        </Resizable.Pane>
      {/if}
    </Resizable.PaneGroup>
  {/if}

  <footer class="flex h-8 shrink-0 items-center justify-between border-t px-3 text-xs text-muted-foreground">
    <span>{#await appVersion() then version}{t("common.version", { version })}{:catch}{/await}</span>
    <span class="min-w-0 flex-1 truncate px-3" role="status">
      {#if problems.length > 0}
        <span title={problems.join("\n")}>{problems.length > 1 ? t("common.status.more", { problem: problems[0], count: problems.length - 1 }) : problems[0]}</span>
      {:else if agent.running}
        <LoaderCircle class="inline size-3 animate-spin" /> {t("common.status.assistant", { activity: agent.activity ?? t("common.status.working") })}
      {:else if doc.downloading}
        {t("common.status.downloading", { name: doc.downloading })}
      {:else if doc.failure}
        {t("common.status.previewFailed", { reason: doc.failure })}
      {/if}
    </span>
    {#if mode === "edit" && layout === "edit"}
      <Button
        variant={assistantOpen ? "secondary" : "ghost"}
        size="icon-xs"
        aria-label={assistantLabel}
        aria-pressed={assistantOpen}
        title={assistantLabel}
        onclick={() => (assistantOpen = !assistantOpen)}
      >
        <Sparkles />
      </Button>
    {/if}
    <Button variant="ghost" size="icon-xs" aria-label={themeLabel} title={themeLabel} onclick={() => settings.set("theme", dark ? "light" : "dark")}>
      {#if dark}<Sun />{:else}<Moon />{/if}
    </Button>
  </footer>
</div>

<SettingsDialog bind:open={settingsOpen} bind:section={settingsSection} />

{#if projects.choosing}<ProjectStart />{/if}

{#if settings.loaded && privacy.status !== null && !projects.choosing}
  {#if !settings.values.setupAsked}
    <FirstStart />
  {:else if privacy.locked && (settings.values.privacyVeil || settings.values.knowledgeOn) && !privacy.unlockDismissed}
    <KnowledgeUnlock />
  {/if}
{/if}

<ProjectNotices />
