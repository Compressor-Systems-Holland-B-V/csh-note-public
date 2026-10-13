<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { longReply, render } from "$lib/markdown";

  let { text, live }: { text: string; live: boolean } = $props();

  let expanded = $state(false);

  const long = $derived(!live && longReply(text));
</script>

<div
  class="min-w-0 wrap-anywhere [&_blockquote]:border-l-2 [&_blockquote]:pl-2 [&_code]:rounded-sm [&_code]:bg-muted [&_code]:px-1 [&_code]:font-mono [&_code]:text-xs [&_li]:my-0.5 [&_ol]:list-decimal [&_ol]:pl-5 [&_pre]:rounded-md [&_pre]:whitespace-pre-wrap [&_pre]:bg-muted [&_pre]:p-2 [&_ul]:list-disc [&_ul]:pl-5 [&>*+*]:mt-2 {long && !expanded
    ? 'max-h-40 overflow-hidden mask-b-from-60%'
    : ''}"
>
  {@html render(text)}
</div>
{#if long}
  <Button variant="link" size="xs" class="justify-self-start px-0" aria-expanded={expanded} onclick={() => (expanded = !expanded)}>
    {expanded ? "Show less" : "Show more"}
  </Button>
{/if}
