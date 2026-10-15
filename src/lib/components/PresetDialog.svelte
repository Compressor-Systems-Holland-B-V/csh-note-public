<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import { Button } from "@zsuite/ui/button";
  import * as Dialog from "@zsuite/ui/dialog";
  import { Input } from "@zsuite/ui/input";
  import { Label } from "@zsuite/ui/label";
  import { Switch } from "@zsuite/ui/switch";
  import { accesses, builtIn, efforts, movePreset, presetName, presetsFor, providerName, providerOf, providers } from "$lib/assistant";
  import type { Preset } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import Picker from "./Picker.svelte";
  import { t } from "$lib/i18n.svelte";

  let { open = $bindable() }: { open: boolean } = $props();

  const provider = $derived(agent.providerOrDefault);
  const providerLabel = $derived(providerName(provider));
  const saved = $derived(settings.values.assistantPresets[provider] ?? []);
  const preset = $derived(agent.preset);
  const standard = $derived(builtIn(provider).find(({ id }) => id === preset.id));
  const changed = $derived(saved.some(({ id }) => id === preset.id));
  const offersFast = $derived(providerOf(provider)?.fast === true);
  const models = $derived(connections.offered(provider));
  const usual = $derived(connections.model(provider));
  const linked = $derived(providers.map(({ id }) => id).filter((id) => id === provider || connections.connected(id)));
  const offers = $derived([
    { provider, model: null as string | null, label: usual === null ? t("assistant.preset.defaultModel") : `${t("assistant.preset.defaultModel")} (${models.find(({ id }) => id === usual)?.label ?? usual})` },
    ...linked.flatMap((id) =>
      connections.offered(id).map((model) => ({ provider: id, model: model.id as string | null, label: linked.length > 1 ? `${providerName(id)} · ${model.label}` : model.label })),
    ),
  ]);

  const choices = (list: { id: string; label: string }[], none: string) => [
    { value: null, label: none },
    ...list.map(({ id, label }) => ({ value: id as string | null, label })),
  ];

  function store(list: Preset[]) {
    settings.set("assistantPresets", { ...settings.values.assistantPresets, [provider]: list });
  }

  function change(patch: Partial<Preset>) {
    store([...saved.filter(({ id }) => id !== preset.id), { ...preset, ...patch }]);
  }

  function pick({ provider: next, model }: (typeof offers)[number]) {
    settings.set("assistantPresets", movePreset(settings.values.assistantPresets, provider, next, { ...preset, model }));
    if (next === provider) return;
    agent.choice = null;
    connections.use(next);
  }

  function add() {
    const id = crypto.randomUUID();
    store([...saved, { ...preset, id, name: t("assistant.preset.mine"), hint: "" }]);
    settings.set("assistantPreset", id);
  }

  function discard() {
    store(saved.filter(({ id }) => id !== preset.id));
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t("assistant.preset.title")}</Dialog.Title>
      <Dialog.Description>
        {t("assistant.preset.text", { provider: providerLabel })}
      </Dialog.Description>
    </Dialog.Header>
    <div class="flex items-center gap-2">
      <Picker
        class="min-w-0 flex-1"
        aria-label={t("assistant.preset.label")}
        value={preset.id}
        options={presetsFor(provider, saved).map(({ id, name }) => ({ value: id, label: presetName({ id, name }) }))}
        onchange={(id) => settings.set("assistantPreset", id)}
      />
      <Button variant="outline" class="shrink-0" onclick={add}><Plus /> {t("assistant.preset.new")}</Button>
    </div>
    <div class="grid grid-cols-1 items-center gap-x-6 gap-y-4 *:min-w-0 sm:grid-cols-[minmax(0,1fr)_18rem]">
      {#if !standard}
        <Label for="preset-name">{t("common.name")}</Label>
        <Input id="preset-name" maxlength={24} value={preset.name} onchange={(event) => change({ name: event.currentTarget.value.trim() || t("assistant.preset.mine") })} />
      {/if}

      <Label for="preset-model">{t("assistant.preset.model")}</Label>
      <div class="grid gap-2">
        {#if offers.length > 1}
          <Picker
            id="preset-model"
            class="w-full"
            value={offers.find((offer) => offer.provider === provider && offer.model === preset.model)}
            options={offers.map((offer) => ({ value: offer as typeof offer | undefined, label: offer.label }))}
            onchange={(offer) => offer && pick(offer)}
          />
        {/if}
        {#if models.length === 0}
          <Input id={offers.length > 1 ? undefined : "preset-model"} placeholder={usual ?? ""} value={preset.model ?? ""} onchange={(event) => change({ model: event.currentTarget.value.trim() || null })} />
        {/if}
      </div>

      <div>
        <Label for="preset-effort">{t("assistant.preset.effort")}</Label>
        <p class="text-xs text-muted-foreground">{t("assistant.preset.effortHint")}</p>
      </div>
      <Picker id="preset-effort" class="w-full" value={preset.effort} options={choices(efforts.map((id) => ({ id, label: t(`assistant.effort.${id}`) })), t("common.default"))} onchange={(effort) => change({ effort })} />

      <Label for="preset-access">{t("assistant.preset.access")}</Label>
      <Picker
        id="preset-access"
        class="w-full"
        value={preset.access}
        options={accesses.map((id) => ({ value: id, label: t(`assistant.access.${id}`) }))}
        onchange={(access) => change({ access })}
      />

      <Label for="preset-web">{t("assistant.preset.web")}</Label>
      <Picker
        id="preset-web"
        class="w-full"
        value={preset.web}
        options={[
          { value: null, label: t("assistant.preset.webAsk") },
          { value: true, label: t("assistant.preset.webOn") },
          { value: false, label: t("assistant.preset.webOff") },
        ]}
        onchange={(web) => change({ web })}
      />

      {#if offersFast}
        <div>
          <Label for="preset-fast">{t("assistant.preset.fast")}</Label>
          <p class="text-xs text-muted-foreground">{t("assistant.preset.fastHint")}</p>
        </div>
        <Switch id="preset-fast" class="justify-self-end" checked={preset.fast} onCheckedChange={(fast) => change({ fast })} />
      {/if}
    </div>
    <Dialog.Footer>
      {#if standard && changed}
        <Button variant="outline" onclick={discard}>{t("assistant.preset.backToStandard", { name: presetName(standard) })}</Button>
      {:else if !standard}
        <Button variant="destructive" onclick={discard}>{t("assistant.preset.delete")}</Button>
      {/if}
      <Button onclick={() => (open = false)}>{t("common.done")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
