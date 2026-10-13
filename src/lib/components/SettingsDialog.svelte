<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import * as NativeSelect from "$lib/components/ui/native-select";
  import { Switch } from "$lib/components/ui/switch";
  import { pickFolder } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { fontSizes, settings } from "$lib/state/settings.svelte";
  import { workspace } from "$lib/state/workspace.svelte";

  let { open = $bindable() }: { open: boolean } = $props();

  const zooms = [0.5, 0.75, 1, 1.25, 1.5, 2];

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
      <NativeSelect.Root
        id="settings-zoom"
        value={String(settings.values.zoom ?? "fit")}
        onchange={(event) => {
          const chosen = event.currentTarget.value;
          settings.set("zoom", chosen === "fit" ? null : Number(chosen));
        }}
      >
        <NativeSelect.Option value="fit">Fit width</NativeSelect.Option>
        {#each zooms as zoom (zoom)}
          <NativeSelect.Option value={String(zoom)}>{zoom * 100}%</NativeSelect.Option>
        {/each}
      </NativeSelect.Root>

      <Label for="settings-theme">Appearance</Label>
      <NativeSelect.Root
        id="settings-theme"
        value={settings.values.theme}
        onchange={(event) => settings.set("theme", event.currentTarget.value === "dark" ? "dark" : "light")}
      >
        <NativeSelect.Option value="light">Light</NativeSelect.Option>
        <NativeSelect.Option value="dark">Dark</NativeSelect.Option>
      </NativeSelect.Root>

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
