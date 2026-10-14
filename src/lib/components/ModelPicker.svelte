<script lang="ts">
  import * as Select from "$lib/components/ui/select";
  import { listsModels, providerName, providers } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import { t } from "$lib/i18n.svelte";

  let { class: className = "" }: { class?: string } = $props();

  const options = $derived(
    providers
      .filter(({ id }) => connections.connected(id))
      .flatMap(({ id }) => {
        const models = listsModels(id) ? connections.offered(id) : [];
        return models.length > 0
          ? models.map((model) => ({ provider: id, model: model.id as string | null, label: model.label }))
          : [{ provider: id, model: null, label: (listsModels(id) ? connections.model(id) : null) ?? providerName(id) }];
      }),
  );
  const several = $derived(new Set(options.map(({ provider }) => provider)).size > 1);
  const chosen = $derived(options.findIndex(({ provider, model }) => provider === agent.using && (model === null || model === agent.model)));

  function choose(index: string) {
    const { provider, model } = options[Number(index)];
    agent.choice = { provider, model };
  }
</script>

{#if options.length > 1}
  <Select.Root type="single" value={String(chosen)} onValueChange={choose} disabled={agent.running}>
    <Select.Trigger size="sm" class={className} aria-label={t("assistant.preset.model")} title={t("assistant.preset.model")}>
      <span class="truncate">{options[chosen]?.label ?? agent.model ?? ""}</span>
    </Select.Trigger>
    <Select.Content>
      {#each options as { provider, label }, index (index)}
        {#if several && provider !== options[index - 1]?.provider}<Select.Label>{providerName(provider)}</Select.Label>{/if}
        <Select.Item value={String(index)} {label}>{label}</Select.Item>
      {/each}
    </Select.Content>
  </Select.Root>
{/if}
