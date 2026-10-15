<script lang="ts">
  import Bot from "@lucide/svelte/icons/bot";
  import Eye from "@lucide/svelte/icons/eye";
  import History from "@lucide/svelte/icons/history";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import Shield from "@lucide/svelte/icons/shield";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ChatFiles from "$lib/components/ChatFiles.svelte";
  import AgentComposer from "$lib/components/AgentComposer.svelte";
  import AgentConnect from "$lib/components/AgentConnect.svelte";
  import AgentHistory from "$lib/components/AgentHistory.svelte";
  import AgentReply from "$lib/components/AgentReply.svelte";
  import AgentSteps from "$lib/components/AgentSteps.svelte";
  import ReferenceIcon from "$lib/components/ReferenceIcon.svelte";
  import VeiledView from "$lib/components/VeiledView.svelte";
  import { Button } from "@zsuite/ui/button";
  import { tick } from "svelte";
  import type { Seen } from "$lib/ipc";
  import { grouped } from "$lib/assistant";
  import { agent } from "$lib/state/agent.svelte";
  import { privacy } from "$lib/state/privacy.svelte";
  import { connections } from "$lib/state/providers.svelte";
  import { t } from "$lib/i18n.svelte";

  let { wide = false }: { wide?: boolean } = $props();

  let list = $state<HTMLDivElement>();
  let history = $state(false);
  let viewing = $state<Seen[] | null>(null);
  let files = $state(false);
  let now = $state(Date.now());

  const shieldTitle = $derived(
    [
      t(agent.hiding ? (privacy.locked ? "privacy.panel.locked" : "privacy.panel.hidden") : "privacy.panel.notHidden"),
      t(!privacy.hiding ? "privacy.panel.offInSettings" : privacy.locked && agent.hiding ? "privacy.panel.lockedTitle" : agent.hiding ? "privacy.panel.hiddenTitle" : "privacy.panel.notHiddenTitle"),
      ...(privacy.hiding && agent.veilLocked ? [t("privacy.panel.fixed")] : []),
      ...(privacy.required("hideNames") ? [t("privacy.panel.required")] : []),
    ].join("\n"),
  );

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
    <span class="font-medium">{t("assistant.title")}</span>
    <Button
      variant="ghost"
      size="icon-xs"
      class={agent.hiding ? "text-chart-2 hover:text-chart-2" : "text-muted-foreground"}
      aria-label={t("privacy.panel.toggle")}
      aria-pressed={agent.hiding}
      title={shieldTitle}
      disabled={!privacy.hiding || agent.veilLocked || privacy.required("hideNames")}
      onclick={() => (agent.veil = !agent.veil)}
    >
      {#if agent.hiding}<ShieldCheck />{:else}<Shield />{/if}
    </Button>
    <Button variant="ghost" size="icon-xs" aria-label={t("assistant.files.title")} title={t("assistant.files.title")} disabled={!agent.hiding} onclick={() => (files = true)}>
      <Eye />
    </Button>
    <span class="flex-1"></span>
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
    <div bind:this={list} class="flex min-h-0 flex-1 flex-col gap-5 overflow-x-hidden overflow-y-auto py-5 leading-relaxed [scrollbar-gutter:stable] {wide ? 'px-[max(1.25rem,calc(50%-24rem))] text-base' : 'px-5'}" aria-live="polite">
      {#each agent.messages as message, index (index)}
        {@const live = agent.here && index === agent.messages.length - 1}
        {#if message.from === "you"}
          <div class="ms-6 flex min-w-0 shrink-0 animate-in flex-col gap-2 rounded-lg bg-accent px-4 py-3 duration-300 fade-in slide-in-from-bottom-2">
            {#each message.parts as part, position (position)}
              {#if part.kind === "note"}
                <p class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground"><ReferenceIcon label={part.text} /> <span class="truncate" title={part.text}>{part.text}</span></p>
              {:else}
                <p class="min-w-0 wrap-anywhere whitespace-pre-wrap">{part.text}</p>
              {/if}
            {/each}
            {#if message.hidden !== undefined}
              {@const saw = agent.messages[index + 1]?.saw ?? []}
              <p class="flex items-center gap-1 text-xs text-chart-2">
                <ShieldCheck class="size-3" role="img" aria-label={t("privacy.panel.hidden")} />
                {#if saw.length > 0}
                  <Button variant="ghost" size="icon-xs" aria-label={t("privacy.preview")} title={t("privacy.preview")} onclick={() => (viewing = saw)}>
                    <Eye />
                  </Button>
                {/if}
              </p>
            {/if}
          </div>
        {:else}
          {@const groups = grouped(message.parts)}
          <div class="flex min-w-0 shrink-0 flex-col gap-2">
            {#each groups as group, position (position)}
              {#if group.kind === "steps"}
                <AgentSteps steps={group.steps} {live} />
              {:else}
                <AgentReply text={group.text} live={live && position === groups.length - 1} restored={message.restored} />
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
      {#if agent.hiding && privacy.blocked}<p class="shrink-0 rounded-md bg-destructive/10 p-2 text-xs text-destructive" role="alert">{t(`privacy.blocked.${privacy.blocked}`)}</p>{/if}
      {#if agent.otherKnowledge}<p class="shrink-0 rounded-md bg-muted p-2 text-xs" role="note">{t("privacy.otherKnowledge")}</p>{/if}
      {#if agent.failure}<p class="shrink-0 wrap-anywhere text-destructive" role="alert">{agent.failure}</p>{/if}
    </div>
    <AgentComposer {wide} />
  {/if}
</aside>
{#if files}<ChatFiles onclose={() => (files = false)} />{/if}
{#if viewing}{@const saw = viewing}<VeiledView load={async () => saw} onclose={() => (viewing = null)} />{/if}
