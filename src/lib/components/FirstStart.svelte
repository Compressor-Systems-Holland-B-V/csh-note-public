<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { privacy } from "$lib/state/privacy.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import KnowledgeForm from "./KnowledgeForm.svelte";
  import LayoutPicker from "./LayoutPicker.svelte";
  import ModelDownload from "./ModelDownload.svelte";
  import { t } from "$lib/i18n.svelte";

  type Step = "layout" | "privacy" | "file" | "create" | "import" | "downloads" | "marketplace";

  let step = $state<Step>("layout");
  let wanted = $state({ veil: settings.values.privacyVeil, names: settings.values.privacyNames, knowledge: settings.values.knowledgeOn });

  const needsFile = $derived((wanted.veil || wanted.knowledge) && !privacy.usable);
  const downloading = $derived(Object.keys(privacy.progress).length > 0);
  const downloads = $derived([...(wanted.veil && wanted.names ? (["names"] as const) : []), ...(wanted.knowledge ? (["search"] as const) : [])]);

  function apply() {
    settings.set("privacyVeil", wanted.veil);
    settings.set("knowledgeOn", wanted.knowledge);
    settings.set("privacyNames", wanted.veil && wanted.names && (privacy.status?.names ?? false));
  }

  function afterPrivacy() {
    if (needsFile) return (step = "file");
    apply();
    step = downloads.length > 0 && privacy.usable ? "downloads" : "marketplace";
  }

  function afterFile() {
    apply();
    step = downloads.length > 0 ? "downloads" : "marketplace";
  }

  function finish() {
    settings.set("privacyAsked", true);
    settings.set("setupAsked", true);
  }
</script>

{#snippet toggle(id: "veil" | "names" | "knowledge", label: string, hint: string, disabled = false)}
  <div class="flex items-center justify-between gap-3">
    <div class="min-w-0">
      <Label for="start-{id}">{label}</Label>
      <p class="text-xs text-muted-foreground">{hint}</p>
    </div>
    <Switch id="start-{id}" checked={wanted[id]} {disabled} onCheckedChange={(checked) => (wanted[id] = checked)} />
  </div>
{/snippet}

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) finish();
  }}
>
  <Dialog.Content class={step === "layout" ? "sm:max-w-2xl" : "sm:max-w-lg"}>
    <Dialog.Header>
      <Dialog.Title>{t(`settings.start.${step === "create" || step === "import" ? "file" : step}`)}</Dialog.Title>
      {#if step === "privacy"}
        <Dialog.Description>
          {t("privacy.start.text")}
          <a class="underline" href="https://github.com/ZAL-Analytics/znote#what-the-assistant-sees" target="_blank" rel="noreferrer">{t("privacy.start.how")}</a>
        </Dialog.Description>
      {:else if step === "downloads"}
        <Dialog.Description>{t("privacy.start.downloadsText")}</Dialog.Description>
      {:else if step === "file"}
        <Dialog.Description>{t("privacy.settings.needsFile")}</Dialog.Description>
      {/if}
    </Dialog.Header>

    {#if step === "layout"}
      <LayoutPicker />
      <div class="flex justify-end"><Button onclick={() => (step = "privacy")}>{t("common.next")}</Button></div>
    {:else if step === "privacy"}
      <div class="grid gap-4">
        {@render toggle("veil", t("privacy.settings.privacyVeil"), t("privacy.settings.privacyVeilHint"))}
        {@render toggle("names", t("privacy.settings.privacyNames"), t("privacy.settings.privacyNamesHint"), !wanted.veil)}
        {@render toggle("knowledge", t("privacy.settings.knowledgeOn"), t("privacy.settings.knowledgeOnHint"))}
      </div>
      <div class="flex justify-between">
        <Button variant="ghost" onclick={() => (step = "layout")}>{t("common.back")}</Button>
        <Button onclick={afterPrivacy}>{t("common.next")}</Button>
      </div>
    {:else if step === "file"}
      <div class="grid gap-2">
        <Button onclick={() => (step = "create")}>{t("privacy.start.create")}</Button>
        <Button variant="outline" onclick={() => (step = "import")}>{t("privacy.start.import")}</Button>
        <Button variant="ghost" onclick={() => (step = "privacy")}>{t("common.back")}</Button>
      </div>
    {:else if step === "create" || step === "import"}
      <KnowledgeForm mode={step} ondone={afterFile} oncancel={() => (step = "file")} />
    {:else if step === "downloads"}
      <div class="grid gap-3">
        {#each downloads as id (id)}<ModelDownload {id} />{/each}
        {#if privacy.failure}<p class="text-sm wrap-anywhere text-destructive" role="alert">{privacy.failure}</p>{/if}
        <div class="flex justify-end"><Button disabled={downloading} onclick={() => (step = "marketplace")}>{t("common.next")}</Button></div>
      </div>
    {:else}
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0">
          <Label for="start-marketplace">{t("settings.marketplace")}</Label>
          <p class="text-xs text-muted-foreground">{t("settings.marketplaceHint")}</p>
        </div>
        <Switch id="start-marketplace" checked={settings.values.marketplace} onCheckedChange={(checked) => settings.set("marketplace", checked)} />
      </div>
      <p class="text-xs text-muted-foreground">{t("privacy.start.later")}</p>
      <div class="flex justify-between">
        <Button variant="ghost" onclick={() => (step = "privacy")}>{t("common.back")}</Button>
        <Button onclick={finish}>{t("common.finish")}</Button>
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>
