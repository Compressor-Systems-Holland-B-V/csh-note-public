<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { providerName, providers } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import ProviderSetup from "./ProviderSetup.svelte";
  import { t } from "$lib/i18n.svelte";

  let { open = $bindable() }: { open: boolean } = $props();

  let shown = $state<string | null>(null);

  $effect(() => {
    if (open) connections.check();
  });
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>{t("providers.dialogTitle")}</Dialog.Title>
      <Dialog.Description>{t("providers.dialogText")}</Dialog.Description>
    </Dialog.Header>
    <ul class="grid gap-2">
      {#each providers as provider (provider.id)}
        {@const connected = connections.status[provider.id]?.connected ?? false}
        {@const active = connections.active === provider.id}
        <li class="rounded-lg border p-3 {active ? 'border-ring' : ''}">
          <div class="flex items-center gap-2">
            <div class="min-w-0 flex-1">
              <p class="font-medium">{providerName(provider.id)}</p>
              <p class="text-xs text-muted-foreground">{active ? t("common.default") : connected ? t("providers.connected") : t(`providers.list.${provider.id}Hint`)}</p>
            </div>
            {#if connected && !active}
              <Button variant="outline" size="sm" disabled={agent.running} onclick={() => connections.use(provider.id)}>{t("providers.use")}</Button>
            {/if}
            <Button
              variant={shown === provider.id ? "secondary" : "ghost"}
              size="sm"
              aria-expanded={shown === provider.id}
              onclick={() => (shown = shown === provider.id ? null : provider.id)}
            >
              {t(connected ? "common.settings" : "common.connect")}
            </Button>
          </div>
          {#if shown === provider.id}
            <div class="mt-3 border-t pt-3"><ProviderSetup {provider} /></div>
          {/if}
        </li>
      {/each}
    </ul>
  </Dialog.Content>
</Dialog.Root>
