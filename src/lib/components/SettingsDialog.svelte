<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { providerName, providers } from "$lib/assistant";
  import { projects } from "$lib/state/project.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import { fontSizes, settings } from "$lib/state/settings.svelte";
  import { theme } from "$lib/state/theme.svelte";
  import Picker from "./Picker.svelte";
  import ProviderDialog from "./ProviderDialog.svelte";
  import { locales, t } from "$lib/i18n.svelte";

  let { open = $bindable() }: { open: boolean } = $props();

  let providersOpen = $state(false);

  const provider = $derived(providers.find(({ id }) => id === connections.active));
  const languages = $derived([
    { value: null as string | null, label: t("settings.languageAuto") },
    ...locales.map(({ tag, name }) => ({ value: tag as string | null, label: name })),
  ]);

  const zooms = $derived([
    { value: null, label: t("common.fitWidth") },
    ...[0.5, 0.75, 1, 1.25, 1.5, 2].map((zoom) => ({ value: zoom, label: `${zoom * 100}%` })),
  ]);
  const appearances = $derived([
    { value: "light" as const, label: t("settings.light") },
    { value: "dark" as const, label: t("settings.dark") },
  ]);

  $effect(() => {
    if (open) connections.check();
  });

  function setFontSize(size: number) {
    if (size >= fontSizes.min && size <= fontSizes.max) settings.set("fontSize", size);
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t("common.settings")}</Dialog.Title>
    </Dialog.Header>
    <div class="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-3">
      <div class="min-w-0">
        <Label for="settings-file">{t("settings.file")}</Label>
        <p class="truncate text-xs text-muted-foreground" title={projects.file}>{projects.file}</p>
      </div>
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

      <Label for="settings-font-size">{t("settings.fontSize")}</Label>
      <Input
        id="settings-font-size"
        type="number"
        class="w-20"
        min={fontSizes.min}
        max={fontSizes.max}
        value={settings.values.fontSize}
        onchange={(event) => setFontSize(event.currentTarget.valueAsNumber)}
      />

      <Label for="settings-wrap">{t("settings.wrap")}</Label>
      <Switch
        id="settings-wrap"
        checked={settings.values.wordWrap}
        onCheckedChange={(checked) => settings.set("wordWrap", checked)}
      />

      <div>
        <Label for="settings-autosave">{t("settings.autosave")}</Label>
        <p class="text-xs text-muted-foreground">{t("settings.autosaveHint")}</p>
      </div>
      <Switch
        id="settings-autosave"
        checked={settings.values.autosave}
        onCheckedChange={(checked) => settings.set("autosave", checked)}
      />

      <Label for="settings-zoom">{t("settings.zoom")}</Label>
      <Picker id="settings-zoom" value={settings.values.zoom} options={zooms} onchange={(zoom) => settings.set("zoom", zoom)} />

      <Label for="settings-theme">{t("settings.appearance")}</Label>
      <Picker
        id="settings-theme"
        value={settings.values.theme}
        options={appearances}
        onchange={(appearance) => settings.set("theme", appearance)}
      />

      <div>
        <Label for="settings-look">{t("settings.look")}</Label>
        <p class="text-xs text-muted-foreground">{t("settings.lookHint")}</p>
      </div>
      <Picker id="settings-look" value={settings.values.uiTheme} options={theme.looks} onchange={(id) => settings.set("uiTheme", id)} />

      <Label for="settings-language">{t("settings.language")}</Label>
      <Picker id="settings-language" value={settings.values.language} options={languages} onchange={(language) => settings.set("language", language)} />

      <div>
        <Label for="settings-assistant">{t("settings.assistant")}</Label>
        <p class="text-xs text-muted-foreground">
          {provider ? t("settings.uses", { provider: providerName(provider.id) }) : t("settings.notConnected")}
        </p>
      </div>
      <Button id="settings-assistant" variant="outline" size="sm" onclick={() => (providersOpen = true)}>{t("settings.providers")}</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<ProviderDialog bind:open={providersOpen} />
