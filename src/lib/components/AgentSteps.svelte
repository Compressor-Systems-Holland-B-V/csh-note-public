<script lang="ts">
  import Brain from "@lucide/svelte/icons/brain";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import GitCompare from "@lucide/svelte/icons/git-compare";
  import { Button } from "$lib/components/ui/button";
  import { summaryOf } from "$lib/assistant";
  import type { ChatPart } from "$lib/ipc";
  import { agent } from "$lib/state/agent.svelte";
  import { t, translated } from "$lib/i18n.svelte";

  let { steps, live }: { steps: ChatPart[]; live: boolean } = $props();

  let open = $state<number[]>([]);
</script>

{#snippet list()}
  <ul class="flex min-w-0 flex-col gap-1">
    {#each steps as { kind, text, page }, position (position)}
      <li class="flex min-w-0 animate-in items-start gap-1.5 duration-300 fade-in slide-in-from-start-1">
        {#if kind === "thought"}
          <Brain class="mt-0.5 size-3 shrink-0" aria-label={t("assistant.steps.thought")} />
          {@const expanded = open.includes(position)}
          <div class="grid min-w-0">
            <span class="min-w-0 italic {expanded ? 'whitespace-pre-wrap' : 'line-clamp-2'}">{text}</span>
            {#if text.length > 120 || text.includes("\n")}
              <Button
                variant="link"
                size="xs"
                class="justify-self-start px-0"
                aria-expanded={expanded}
                onclick={() => (open = expanded ? open.filter((other) => other !== position) : [...open, position])}
              >
                {t(expanded ? "assistant.showLess" : "assistant.showMore")}
              </Button>
            {/if}
          </div>
        {:else}
          {@const said = translated(text, "assistant.note")}
          <Check class="mt-0.5 size-3 shrink-0 text-chart-2" aria-hidden="true" />
          <span class="min-w-0 flex-1 truncate" title={said}>{said}</span>
          {#if page !== undefined && page in agent.pending}
            <button
              type="button"
              class="flex shrink-0 items-center gap-1 rounded-sm px-1 text-foreground underline-offset-2 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
              title={t("assistant.steps.reviewTitle")}
              onclick={() => agent.review(page)}
            >
              <GitCompare class="size-3" /> {t("assistant.steps.review")}
            </button>
          {/if}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#if live}
  <div class="text-xs text-muted-foreground">{@render list()}</div>
{:else}
  <details class="group text-xs text-muted-foreground">
    <summary class="flex cursor-default list-none items-center gap-1 rounded-sm outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
      <ChevronRight class="size-3 shrink-0 transition-transform group-open:rotate-90 rtl:rotate-180 rtl:group-open:rotate-90" />
      <span class="truncate">{summaryOf(steps)}</span>
    </summary>
    <div class="mt-1 ps-4">{@render list()}</div>
  </details>
{/if}
