<script lang="ts">
  import Bot from "@lucide/svelte/icons/bot";
  import FileText from "@lucide/svelte/icons/file-text";
  import History from "@lucide/svelte/icons/history";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import AgentComposer from "$lib/components/AgentComposer.svelte";
  import AgentConnect from "$lib/components/AgentConnect.svelte";
  import AgentHistory from "$lib/components/AgentHistory.svelte";
  import AgentReply from "$lib/components/AgentReply.svelte";
  import AgentSteps from "$lib/components/AgentSteps.svelte";
  import { Button } from "$lib/components/ui/button";
  import { tick } from "svelte";
  import { grouped } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import { t } from "$lib/i18n.svelte";

  let list = $state<HTMLDivElement>();
  let history = $state(false);
  let now = $state(Date.now());

  const seconds = $derived(agent.started === null ? 0 : Math.max(0, Math.round((now - agent.started) / 1000)));

  $effect(() => {
    connections.check();
  });

  $effect(() => {
    if (agent.started === null) return;
    now = Date.now();
    const clock = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(clock);
  });

  $effect.pre(() => {
    void [agent.messages.length, agent.messages.at(-1)?.parts.length, agent.messages.at(-1)?.parts.at(-1)?.text, agent.deletion, agent.activity, agent.failure];
    if (!list || list.scrollHeight - list.scrollTop - list.clientHeight > 80) return;
    tick().then(() => list?.scrollTo({ top: list.scrollHeight }));
  });
</script>

<aside class="flex h-full min-h-0 flex-col overflow-hidden text-sm" aria-label={t("assistant.title")}>
  <div class="relative flex h-9 shrink-0 items-center gap-1 border-b px-2">
    <span class="flex-1 font-medium">{t("assistant.title")}</span>
    {#if connections.ready}
      <Button
        variant={history ? "secondary" : "ghost"}
        size="icon-xs"
        aria-label={t("assistant.history.title")}
        aria-pressed={history}
        title={t("assistant.history.title")}
        onclick={() => (history = !history)}
      >
        <History />
      </Button>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={t("assistant.history.newChat")}
        title={t("assistant.history.newChat")}
        onclick={() => {
          history = false;
          agent.newChat();
        }}
      >
        <MessageSquarePlus />
      </Button>
    {/if}
    {#if agent.running}<span class="absolute inset-x-0 -bottom-px h-0.5 animate-pulse bg-chart-2" aria-hidden="true"></span>{/if}
  </div>

  {#if !connections.checked}
    <p class="flex flex-1 items-center justify-center p-4 text-center text-muted-foreground" role="status">
      {connections.failure ?? t("assistant.checking")}
    </p>
  {:else if !connections.ready}
    <AgentConnect />
  {:else if history}
    <AgentHistory onopen={() => (history = false)} />
  {:else}
    <div bind:this={list} class="flex min-h-0 flex-1 flex-col gap-5 overflow-x-hidden overflow-y-auto px-5 py-5 leading-relaxed [scrollbar-gutter:stable]" aria-live="polite">
      {#each agent.messages as message, index (index)}
        {@const live = agent.here && index === agent.messages.length - 1}
        {#if message.from === "you"}
          <div class="ms-6 flex min-w-0 shrink-0 animate-in flex-col gap-2 rounded-lg bg-accent px-4 py-3 duration-300 fade-in slide-in-from-bottom-2">
            {#each message.parts as part, position (position)}
              {#if part.kind === "note"}
                <p class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground"><FileText class="size-3 shrink-0" /> <span class="truncate" title={part.text}>{part.text}</span></p>
              {:else}
                <p class="min-w-0 wrap-anywhere whitespace-pre-wrap">{part.text}</p>
              {/if}
            {/each}
          </div>
        {:else}
          {@const groups = grouped(message.parts)}
          <div class="flex min-w-0 shrink-0 flex-col gap-2">
            {#each groups as group, position (position)}
              {#if group.kind === "steps"}
                <AgentSteps steps={group.steps} {live} />
              {:else}
                <AgentReply text={group.text} live={live && position === groups.length - 1} />
              {/if}
            {/each}
          </div>
        {/if}
      {:else}
        <div class="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center">
          <Bot class="size-10 text-muted-foreground" aria-hidden="true" />
          <p class="font-medium">{t("assistant.empty.title")}</p>
        </div>
      {/each}
      {#if agent.deletion}
        {@const { path, answer } = agent.deletion}
        <div class="flex shrink-0 animate-in flex-col gap-3 rounded-lg border p-4 duration-300 zoom-in-95 fade-in" role="alertdialog" aria-label={t("assistant.deletion.label")}>
          <p>{t("assistant.deletion.text", { path })}</p>
          <div class="flex gap-1">
            <Button variant="destructive" size="sm" onclick={() => answer(true)}>{t("common.delete")}</Button>
            <Button variant="outline" size="sm" onclick={() => answer(false)}>{t("common.keep")}</Button>
          </div>
        </div>
      {:else if agent.here && agent.activity}
        <p class="flex min-w-0 shrink-0 items-center gap-2" role="status">
          <LoaderCircle class="size-4 shrink-0 animate-spin text-chart-2" />
          {#key agent.activity}
            <span class="shimmer min-w-0 flex-1 animate-in truncate duration-300 fade-in slide-in-from-bottom-1">{agent.activity}</span>
          {/key}
          <span class="shrink-0 text-xs text-muted-foreground tabular-nums" aria-label={t("assistant.timeSoFar")}>{t("assistant.seconds", { seconds })}</span>
        </p>
      {/if}
      {#if agent.failure}<p class="shrink-0 wrap-anywhere text-destructive" role="alert">{agent.failure}</p>{/if}
    </div>
    <AgentComposer />
  {/if}
</aside>
