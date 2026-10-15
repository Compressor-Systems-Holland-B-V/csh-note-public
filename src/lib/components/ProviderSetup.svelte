<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import UserRound from "@lucide/svelte/icons/user-round";
  import { Button } from "@zsuite/ui/button";
  import { Input } from "@zsuite/ui/input";
  import { Label } from "@zsuite/ui/label";
  import type { Provider } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import Picker from "./Picker.svelte";
  import { t } from "$lib/i18n.svelte";

  let { provider }: { provider: Provider } = $props();

  let code = $state("");
  let copied = $state(false);
  let url = $state("");
  let key = $state("");

  const status = $derived(connections.status[provider.id]);
  const models = $derived(connections.offered(provider.id));
  const model = $derived(connections.model(provider.id));

  const ways = [
    { method: null, id: "subscription", icon: UserRound },
    { method: "console", id: "console", icon: KeyRound },
    { method: "sso", id: "sso", icon: ShieldCheck },
  ];

  async function copy(link: string) {
    await navigator.clipboard.writeText(link);
    copied = true;
  }

  function sendCode(event: SubmitEvent) {
    event.preventDefault();
    connections.sendCode(code);
    code = "";
  }

  async function connect(event: SubmitEvent) {
    event.preventDefault();
    await connections.connect(provider.id, provider.needs === "server" ? url : null, key);
    key = "";
  }
</script>

<div class="grid w-full gap-3 text-start">
  {#if status?.connected}
    <p class="flex items-center gap-2 text-sm"><Check class="size-4 text-chart-2" /> {status.url && provider.needs === "server" ? t("providers.connectedTo", { url: status.url }) : t("providers.connected")}</p>
    {#if provider.needs !== "sign-in"}
      <div class="grid gap-1">
        <Label for="model-{provider.id}">{t("assistant.preset.defaultModel")}</Label>
        {#if models.length > 0}
          <Picker
            id="model-{provider.id}"
            class="w-full"
            value={model}
            options={models.map(({ id, label }) => ({ value: id as string | null, label }))}
            onchange={(id) => connections.setModel(provider.id, id ?? "")}
          />
        {:else}
          <Input
            id="model-{provider.id}"
            placeholder={t("providers.model.type")}
            value={model ?? ""}
            aria-invalid={model === null}
            onchange={(event) => connections.setModel(provider.id, event.currentTarget.value)}
          />
        {/if}
        <p class="text-xs text-muted-foreground">
          {t(model === null ? "providers.model.missing" : "providers.model.hint")}
        </p>
      </div>
    {/if}
    <Button variant="outline" size="sm" class="justify-self-start" disabled={agent.running} onclick={() => connections.disconnect(provider.id)}>
      {t("providers.disconnect")}
    </Button>
  {:else if provider.needs === "sign-in"}
    {#if connections.stage}
      <p class="flex items-center gap-2 text-sm" role="status">
        <LoaderCircle class="size-4 animate-spin" />
        {t(connections.stage === "installing" ? "providers.signIn.installing" : "providers.signIn.finish")}
      </p>
      {#if connections.stage === "connecting" && connections.link}
        {@const link = connections.link}
        <div class="grid gap-2 border-t pt-3">
          <p class="text-xs text-muted-foreground">
            {t("providers.signIn.noBrowser")}
          </p>
          <div class="flex gap-1">
            <Input readonly value={link} aria-label={t("providers.signIn.link")} class="h-8 text-xs" onfocus={(event) => event.currentTarget.select()} />
            <Button variant="outline" size="icon-sm" aria-label={t("providers.signIn.copyLink")} title={t("providers.signIn.copyLink")} onclick={() => copy(link)}>
              {#if copied}<Check />{:else}<Copy />{/if}
            </Button>
          </div>
          <form class="flex gap-1" onsubmit={sendCode}>
            <Input bind:value={code} aria-label={t("providers.signIn.code")} placeholder={t("providers.signIn.code")} class="h-8 text-xs" autocomplete="off" />
            <Button type="submit" size="sm" disabled={code.trim() === ""}>{t("common.connect")}</Button>
          </form>
        </div>
      {/if}
      <Button variant="outline" size="sm" class="justify-self-start" onclick={() => connections.stop()}>{t("common.cancel")}</Button>
    {:else}
      {#each ways as { method, id, icon: Icon }, index (id)}
        <Button
          variant={index === 0 ? "default" : "outline"}
          class="h-auto justify-start gap-3 py-2 text-start whitespace-normal"
          onclick={() => connections.signIn(method)}
        >
          <Icon class="size-5" />
          <span class="grid">
            <span>{t(`providers.signIn.${id}`)}</span>
            <span class="text-xs font-normal opacity-70">{t(`providers.signIn.${id}Hint`)}</span>
          </span>
        </Button>
      {/each}
    {/if}
    <p class="text-xs text-muted-foreground">{t("providers.signIn.note")}</p>
  {:else}
    <form class="grid gap-2" onsubmit={connect}>
      {#if provider.needs === "server"}
        <div class="grid gap-1">
          <Label for="url-{provider.id}">{t("providers.key.server")}</Label>
          <Input id="url-{provider.id}" bind:value={url} placeholder="http://localhost:11434/v1" autocomplete="off" spellcheck={false} />
        </div>
      {/if}
      <div class="grid gap-1">
        <Label for="key-{provider.id}">{t(provider.needs === "server" ? "providers.key.labelOptional" : "providers.key.label")}</Label>
        <Input id="key-{provider.id}" type="password" bind:value={key} autocomplete="off" spellcheck={false} />
      </div>
      <Button type="submit" size="sm" class="justify-self-start" disabled={connections.stage !== null || (provider.needs === "key" ? key.trim() === "" : url.trim() === "")}>
        {#if connections.stage}<LoaderCircle class="animate-spin" /> {t("providers.checking")}{:else}{t("common.connect")}{/if}
      </Button>
    </form>
    <p class="text-xs text-muted-foreground">{t("providers.key.note")}</p>
  {/if}
  {#if connections.failure}<p class="text-sm wrap-anywhere whitespace-pre-line text-destructive" role="alert">{connections.failure}</p>{/if}
</div>
