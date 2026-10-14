<script lang="ts">
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { untrack } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import type { Seen } from "$lib/ipc";
  import { messageOf } from "$lib/utils";
  import PrivacyView from "./PrivacyView.svelte";
  import { t } from "$lib/i18n.svelte";

  let {
    load,
    editable = false,
    level,
    onlevel,
    onclose,
  }: { load: () => Promise<Seen[]>; editable?: boolean; level?: number; onlevel?: (level: number) => void; onclose: () => void } = $props();

  const levels = ["own", "light", "medium", "strict"];

  let shown = $state<Seen[]>([]);
  let failure = $state<string | null>(null);

  const named = (label: string) => (label === "prompt" ? t("privacy.saw.prompt") : label);

  $effect(() => {
    untrack(load).then(
      (seen) => (shown = seen),
      (error: unknown) => (failure = messageOf(error)),
    );
  });
</script>

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) onclose();
  }}
>
  <Dialog.Content class="flex h-[85vh] flex-col sm:max-w-4xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2"><ShieldCheck class="size-5 text-chart-2" /> {t("privacy.preview")}</Dialog.Title>
      <Dialog.Description>{t("privacy.panel.hiddenTitle")}</Dialog.Description>
    </Dialog.Header>
    {#if onlevel && level !== undefined}
      <label class="grid gap-1 text-sm">
        <span class="flex justify-between gap-2"><span class="font-medium">{t("privacy.level.label")}</span><span class="text-muted-foreground">{t(`privacy.level.${levels[level]}`)}</span></span>
        <input type="range" min="0" max={levels.length - 1} step="1" value={level} class="accent-primary" oninput={(event) => onlevel(Number(event.currentTarget.value))} />
      </label>
    {/if}
    <div class="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto">
      {#if failure}<p class="text-sm text-destructive" role="alert">{failure}</p>{/if}
      {#each shown as { label, text }, index (index)}
        <div class="grid gap-1">
          <p class="text-sm font-medium">{named(label)}</p>
          {#if editable}
            <div class="overflow-hidden rounded-md border"><PrivacyView {text} {level} /></div>
          {:else}
            <pre class="rounded-md bg-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap" dir="ltr">{text}</pre>
          {/if}
        </div>
      {/each}
    </div>
  </Dialog.Content>
</Dialog.Root>
