<script lang="ts">
  import Palette from "@lucide/svelte/icons/palette";
  import Settings from "@lucide/svelte/icons/settings";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import SquarePen from "@lucide/svelte/icons/square-pen";
  import { Button } from "@zsuite/ui/button";
  import * as Dialog from "@zsuite/ui/dialog";
  import { Input } from "@zsuite/ui/input";
  import { Label } from "@zsuite/ui/label";
  import { Switch } from "@zsuite/ui/switch";
  import * as Tabs from "@zsuite/ui/tabs";
  import { projects } from "$lib/state/project.svelte";
  import { fontSizes, settings } from "$lib/state/settings.svelte";
  import { theme } from "$lib/state/theme.svelte";
  import LayoutPicker from "./LayoutPicker.svelte";
  import { Picker } from "@zsuite/ui/picker";
  import PrivacySettings from "./PrivacySettings.svelte";
  import ProviderList from "@zsuite/assistant/components/ProviderList";
  import { languageOptions, t } from "$lib/i18n.svelte";

  let { open = $bindable(), section = $bindable("general") }: { open: boolean; section?: string } = $props();

  const sections = $derived([
    { id: "general", icon: Settings, label: t("settings.general") },
    { id: "editor", icon: SquarePen, label: t("settings.editor") },
    { id: "appearance", icon: Palette, label: t("settings.appearance") },
    { id: "assistant", icon: Sparkles, label: t("settings.assistant") },
    { id: "privacy", icon: ShieldCheck, label: t("privacy.settings.title") },
  ]);
  const heading = $derived(sections.find(({ id }) => id === section)?.label);

  const zooms = $derived([
    { value: null, label: t("common.fitWidth") },
    ...[0.5, 0.75, 1, 1.25, 1.5, 2].map((zoom) => ({ value: zoom, label: `${zoom * 100}%` })),
  ]);
  const appearances = $derived([
    { value: "light" as const, label: t("settings.light") },
    { value: "dark" as const, label: t("settings.dark") },
  ]);

  const row = "flex items-center justify-between gap-6 py-3";

  function setFontSize(size: number) {
    if (size >= fontSizes.min && size <= fontSizes.max) settings.set("fontSize", size);
  }
</script>

{#snippet text(id: string, label: string, hint: string | null = null)}
  <div class="min-w-0">
    <Label for={id}>{label}</Label>
    {#if hint}<p class="mt-0.5 text-xs text-muted-foreground">{hint}</p>{/if}
  </div>
{/snippet}

<Dialog.Root bind:open>
  <Dialog.Content class="h-[min(40rem,85vh)] gap-0 overflow-hidden p-0 sm:max-w-3xl">
    <Tabs.Root bind:value={section} orientation="vertical" class="h-full min-h-0 gap-0">
      <div class="flex w-48 shrink-0 flex-col gap-3 border-e bg-sidebar p-3">
        <Dialog.Title class="px-2 pt-1">{t("common.settings")}</Dialog.Title>
        <Tabs.List class="w-full gap-0.5 bg-transparent p-0">
          {#each sections as { id, icon: Icon, label } (id)}
            <Tabs.Trigger value={id} class="h-8 flex-none px-2">
              <Icon /> {label}
            </Tabs.Trigger>
          {/each}
        </Tabs.List>
      </div>
      <div class="min-w-0 flex-1 overflow-y-auto px-6 pt-5 pb-6">
        <h2 class="mb-2 text-base font-medium">{heading}</h2>

        <Tabs.Content value="general" class="divide-y">
          {#if projects.file}
            <div class={row}>
              {@render text("settings-file", t("settings.file"), projects.file)}
              <Button
                id="settings-file"
                variant="outline"
                size="sm"
                onclick={() => {
                  open = false;
                  projects.choosing = true;
                }}
              >
                {t("settings.switch")}
              </Button>
            </div>
          {/if}
          <div class={row}>
            {@render text("settings-language", t("settings.language"))}
            <Picker id="settings-language" value={settings.values.language} options={languageOptions()} onchange={(language) => settings.set("language", language)} />
          </div>
          <div class="grid gap-2 py-3">
            <p class="text-sm font-medium">{t("settings.layout")}</p>
            <LayoutPicker />
          </div>
          <div class={row}>
            {@render text("settings-marketplace", t("settings.marketplace"), t("settings.marketplaceHint"))}
            <Switch id="settings-marketplace" checked={settings.values.marketplace} onCheckedChange={(checked) => settings.set("marketplace", checked)} />
          </div>
          <div class={row}>
            {@render text("settings-autosave", t("settings.autosave"), t("settings.autosaveHint"))}
            <Switch id="settings-autosave" checked={settings.values.autosave} onCheckedChange={(checked) => settings.set("autosave", checked)} />
          </div>
        </Tabs.Content>

        <Tabs.Content value="editor" class="divide-y">
          <div class={row}>
            {@render text("settings-font-size", t("settings.fontSize"))}
            <Input
              id="settings-font-size"
              type="number"
              class="w-20"
              min={fontSizes.min}
              max={fontSizes.max}
              value={settings.values.fontSize}
              onchange={(event) => setFontSize(event.currentTarget.valueAsNumber)}
            />
          </div>
          <div class={row}>
            {@render text("settings-wrap", t("settings.wrap"))}
            <Switch id="settings-wrap" checked={settings.values.wordWrap} onCheckedChange={(checked) => settings.set("wordWrap", checked)} />
          </div>
          <div class={row}>
            {@render text("settings-zoom", t("settings.zoom"))}
            <Picker id="settings-zoom" value={settings.values.zoom} options={zooms} onchange={(zoom) => settings.set("zoom", zoom)} />
          </div>
        </Tabs.Content>

        <Tabs.Content value="appearance" class="divide-y">
          <div class={row}>
            {@render text("settings-theme", t("settings.appearance"))}
            <Picker id="settings-theme" value={settings.values.theme} options={appearances} onchange={(appearance) => settings.set("theme", appearance)} />
          </div>
          <div class={row}>
            {@render text("settings-look", t("settings.look"), t("settings.lookHint"))}
            <Picker id="settings-look" value={settings.values.uiTheme} options={theme.looks} onchange={(id) => settings.set("uiTheme", id)} />
          </div>
        </Tabs.Content>

        <Tabs.Content value="assistant" class="grid gap-4">
          <ProviderList />
        </Tabs.Content>

        <Tabs.Content value="privacy" class="grid gap-4">
          <PrivacySettings />
        </Tabs.Content>
      </div>
    </Tabs.Root>
  </Dialog.Content>
</Dialog.Root>
