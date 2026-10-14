<script lang="ts">
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { privacy } from "$lib/state/privacy.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import KnowledgeForm from "./KnowledgeForm.svelte";
  import ModelDownload from "./ModelDownload.svelte";
  import type { FormMode } from "./KnowledgeForm.svelte";
  import { t } from "$lib/i18n.svelte";

  let step = $state<"ask" | FormMode | "downloads">("ask");
  const downloading = $derived(Object.keys(privacy.progress).length > 0);
</script>

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) settings.set("privacyAsked", true);
  }}
>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2"><ShieldCheck class="size-5" /> {t(step === "downloads" ? "privacy.start.downloadsTitle" : "privacy.start.title")}</Dialog.Title>
      <Dialog.Description>
        {t(step === "downloads" ? "privacy.start.downloadsText" : "privacy.start.text")}
        {#if step !== "downloads"}
          <a class="underline" href="https://github.com/ZAL-Analytics/znote#what-the-assistant-sees" target="_blank" rel="noreferrer">{t("privacy.start.how")}</a>
        {/if}
      </Dialog.Description>
    </Dialog.Header>
    {#if step === "ask"}
      <div class="grid gap-2">
        <Button onclick={() => (step = "create")}>{t("privacy.start.create")}</Button>
        <Button variant="outline" onclick={() => (step = "import")}>{t("privacy.start.import")}</Button>
        <Button variant="ghost" onclick={() => settings.set("privacyAsked", true)}>{t("privacy.start.notNow")}</Button>
      </div>
      <p class="text-xs text-muted-foreground">{t("privacy.start.later")}</p>
    {:else if step === "downloads"}
      <div class="grid gap-3">
        <ModelDownload id="names" />
        <ModelDownload id="search" />
        {#if privacy.failure}<p class="text-sm wrap-anywhere text-destructive" role="alert">{privacy.failure}</p>{/if}
        <div class="flex items-center justify-between gap-3">
          <p class="text-xs text-muted-foreground">{t("privacy.start.later")}</p>
          <Button disabled={downloading} onclick={() => settings.set("privacyAsked", true)}>{t("common.next")}</Button>
        </div>
      </div>
    {:else}
      <KnowledgeForm mode={step} ondone={() => (step = "downloads")} oncancel={() => (step = "ask")} />
    {/if}
  </Dialog.Content>
</Dialog.Root>
