<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { pickFolder } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { fontSizes, settings } from "$lib/state/settings.svelte";
  import { theme } from "$lib/state/theme.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import Picker from "./Picker.svelte";

  let { open = $bindable() }: { open: boolean } = $props();

  const zooms = [
    { value: null, label: "Fit width" },
    ...[0.5, 0.75, 1, 1.25, 1.5, 2].map((zoom) => ({ value: zoom, label: `${zoom * 100}%` })),
  ];
  const appearances = [
    { value: "light" as const, label: "Light" },
    { value: "dark" as const, label: "Dark" },
  ];

  function changeFolder() {
    return workspace.attempt(async () => {
      const folder = await pickFolder();
      if (folder !== null) await workspace.useFolder(folder, false);
    });
  }

  $effect(() => {
    if (open) agent.check();
  });

  function setFontSize(size: number) {
    if (size >= fontSizes.min && size <= fontSizes.max) settings.set("fontSize", size);
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Settings</Dialog.Title>
    </Dialog.Header>
    <div class="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-3">
      <div class="min-w-0">
        <Label for="settings-folder">Folder for your notes</Label>
        <p class="truncate text-xs text-muted-foreground" title={settings.values.workspace}>
          {settings.values.workspace}
        </p>
      </div>
      <Button id="settings-folder" variant="outline" size="sm" onclick={changeFolder}>Change…</Button>

      <Label for="settings-font-size">Editor text size</Label>
      <Input
        id="settings-font-size"
        type="number"
        class="w-20"
        min={fontSizes.min}
        max={fontSizes.max}
        value={settings.values.fontSize}
        onchange={(event) => setFontSize(event.currentTarget.valueAsNumber)}
      />

      <Label for="settings-wrap">Wrap long lines</Label>
      <Switch
        id="settings-wrap"
        checked={settings.values.wordWrap}
        onCheckedChange={(checked) => settings.set("wordWrap", checked)}
      />

      <div>
        <Label for="settings-autosave">Save automatically</Label>
        <p class="text-xs text-muted-foreground">When this is off, press Ctrl+S to save.</p>
      </div>
      <Switch
        id="settings-autosave"
        checked={settings.values.autosave}
        onCheckedChange={(checked) => settings.set("autosave", checked)}
      />

      <Label for="settings-zoom">Preview size at start</Label>
      <Picker id="settings-zoom" value={settings.values.zoom} options={zooms} onchange={(zoom) => settings.set("zoom", zoom)} />

      <Label for="settings-theme">Appearance</Label>
      <Picker
        id="settings-theme"
        value={settings.values.theme}
        options={appearances}
        onchange={(appearance) => settings.set("theme", appearance)}
      />

      <div>
        <Label for="settings-look">Look</Label>
        <p class="text-xs text-muted-foreground">Add more looks on the Themes page.</p>
      </div>
      <Picker id="settings-look" value={settings.values.uiTheme} options={theme.looks} onchange={(id) => settings.set("uiTheme", id)} />

      <div>
        <Label for="settings-assistant">Assistant</Label>
        <p class="text-xs text-muted-foreground">
          Runs Claude Code and uses your own Claude account.
          {agent.account?.connected ? "Connected." : "Not connected."}
        </p>
      </div>
      {#if agent.account?.connected}
        <Button id="settings-assistant" variant="outline" size="sm" disabled={agent.running} onclick={() => agent.disconnect()}>
          Disconnect
        </Button>
      {:else if agent.stage}
        <Button id="settings-assistant" variant="outline" size="sm" onclick={() => agent.stop()}>
          {agent.stage === "installing" ? "Downloading… Cancel" : "Waiting for sign-in… Cancel"}
        </Button>
      {:else}
        <Button id="settings-assistant" variant="outline" size="sm" onclick={() => agent.connect()}>Connect</Button>
      {/if}

      <div>
        <Label for="settings-confirm-deletes">Ask before the assistant deletes</Label>
        <p class="text-xs text-muted-foreground">Deleted items always go to the Recycle Bin.</p>
      </div>
      <Switch
        id="settings-confirm-deletes"
        checked={settings.values.confirmDeletes}
        onCheckedChange={(checked) => settings.set("confirmDeletes", checked)}
      />
    </div>
  </Dialog.Content>
</Dialog.Root>
