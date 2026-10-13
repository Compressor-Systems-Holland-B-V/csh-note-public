<script lang="ts">
  import { untrack } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { longReply, render } from "$lib/markdown";
  import { t } from "$lib/i18n.svelte";

  let { text, live }: { text: string; live: boolean } = $props();

  const renderEveryMs = 100;

  let expanded = $state(false);
  let shown = $state(untrack(() => text));
  let timer: ReturnType<typeof setTimeout> | undefined;

  const long = $derived(!live && longReply(text));

  $effect(() => {
    void text;
    if (live) {
      timer ??= setTimeout(() => {
        timer = undefined;
        shown = text;
      }, renderEveryMs);
      return;
    }
    clearTimeout(timer);
    timer = undefined;
    shown = text;
  });

  $effect(() => () => clearTimeout(timer));
</script>

<div
  class="min-w-0 wrap-anywhere [&_blockquote]:border-s-2 [&_blockquote]:ps-2 [&_code]:rounded-sm [&_code]:bg-muted [&_code]:px-1 [&_code]:font-mono [&_code]:text-xs [&_li]:my-0.5 [&_ol]:list-decimal [&_ol]:ps-5 [&_pre]:rounded-md [&_pre]:whitespace-pre-wrap [&_pre]:bg-muted [&_pre]:p-2 [&_ul]:list-disc [&_ul]:ps-5 [&>*+*]:mt-2 {live
    ? 'typing'
    : ''} {long && !expanded ? 'max-h-40 overflow-hidden mask-b-from-60%' : ''}"
>
  {@html render(shown)}
</div>
{#if long}
  <Button variant="link" size="xs" class="justify-self-start px-0" aria-expanded={expanded} onclick={() => (expanded = !expanded)}>
    {t(expanded ? "assistant.showLess" : "assistant.showMore")}
  </Button>
{/if}
