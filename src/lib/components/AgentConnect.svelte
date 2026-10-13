<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Bot from "@lucide/svelte/icons/bot";
  import { untrack } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { providerName, providers } from "$lib/assistant";
  import { connections } from "$lib/state/providers.svelte";
  import ProviderSetup from "./ProviderSetup.svelte";
  import { t } from "$lib/i18n.svelte";

  let chosen = $state(untrack(() => connections.active));

  const provider = $derived(providers.find(({ id }) => id === chosen));
</script>

<div class="flex min-h-0 flex-1 flex-col items-center gap-3 overflow-y-auto p-4 text-center">
  <Bot class="mt-auto size-10 shrink-0 text-muted-foreground" aria-hidden="true" />
  {#if provider}
    <p class="font-medium">{providerName(provider.id)}</p>
    <div class="w-full max-w-72"><ProviderSetup {provider} /></div>
    <Button variant="ghost" size="sm" class="mb-auto" disabled={connections.stage !== null} onclick={() => (chosen = null)}>
      <ArrowLeft class="rtl:rotate-180" /> {t("providers.chooseElse")}
    </Button>
  {:else}
    <p class="font-medium">{t("providers.connectTitle")}</p>
    <p class="text-muted-foreground">{t("providers.connectText")}</p>
    <div class="mb-auto grid w-full max-w-72 gap-2">
      {#each providers as { id } (id)}
        <Button variant="outline" class="h-auto justify-start py-2 text-start whitespace-normal" onclick={() => (connections.status[id]?.connected ? connections.use(id) : (chosen = id))}>
          <span class="grid">
            <span>{connections.status[id]?.connected ? t("providers.nameConnected", { name: providerName(id) }) : providerName(id)}</span>
            <span class="text-xs font-normal text-muted-foreground">{t(`providers.list.${id}Hint`)}</span>
          </span>
        </Button>
      {/each}
    </div>
  {/if}
</div>
