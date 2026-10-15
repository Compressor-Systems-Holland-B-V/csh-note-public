<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import { Button } from "@zsuite/ui/button";
  import { providerName, providers } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import ProviderSetup from "./ProviderSetup.svelte";
  import { t } from "$lib/i18n.svelte";

  let shown = $state<string | null>(null);

  const allowed = $derived(providers.filter(({ id }) => privacy.policy === null || privacy.policy.providers.includes(id)));

  $effect(() => {
    connections.check();
  });
</script>

<ul class="grid gap-2">
  {#each allowed as provider (provider.id)}
    {@const connected = connections.status[provider.id]?.connected ?? false}
    {@const active = connections.active === provider.id}
    {@const open = shown === provider.id}
    <li class="rounded-lg border {active ? 'border-ring' : ''}">
      <div class="flex items-center gap-3 p-3">
        <span class="size-2 shrink-0 rounded-full {connected ? 'bg-chart-2' : 'bg-muted-foreground/30'}" aria-hidden="true"></span>
        <div class="min-w-0 flex-1">
          <p class="flex items-center gap-2 font-medium">
            {providerName(provider.id)}
            {#if active}<span class="rounded-sm bg-muted px-1.5 py-0.5 text-xs font-normal text-muted-foreground">{t("common.default")}</span>{/if}
          </p>
          <p class="text-xs text-muted-foreground">{connected ? t("providers.connected") : t(`providers.list.${provider.id}Hint`)}</p>
        </div>
        {#if connected && !active}
          <Button variant="outline" size="sm" disabled={agent.running} onclick={() => connections.use(provider.id)}>{t("providers.use")}</Button>
        {/if}
        <Button variant={open ? "secondary" : "ghost"} size="sm" aria-expanded={open} onclick={() => (shown = open ? null : provider.id)}>
          {t(connected ? "common.settings" : "common.connect")}
          <ChevronDown class="transition-transform {open ? 'rotate-180' : ''}" />
        </Button>
      </div>
      {#if open}
        <div class="border-t bg-muted/30 p-3"><ProviderSetup {provider} /></div>
      {/if}
    </li>
  {/each}
</ul>
