<script lang="ts">
  import Bot from "@lucide/svelte/icons/bot";
  import FileText from "@lucide/svelte/icons/file-text";
  import History from "@lucide/svelte/icons/history";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import AgentComposer from "$lib/components/AgentComposer.svelte";
  import AgentConnect from "$lib/components/AgentConnect.svelte";
  import AgentHistory from "$lib/components/AgentHistory.svelte";
  import AgentReply from "$lib/components/AgentReply.svelte";
  import { Button } from "$lib/components/ui/button";
  import { tick } from "svelte";
  import { agent, visible } from "$lib/state/agent.svelte";

  let list = $state<HTMLDivElement>();
  let history = $state(false);

  $effect(() => {
    agent.check();
  });

  $effect.pre(() => {
    void [agent.messages.length, agent.messages.at(-1)?.parts.at(-1)?.text, agent.deletion, agent.activity, agent.failure];
    if (!list || list.scrollHeight - list.scrollTop - list.clientHeight > 80) return;
    tick().then(() => list?.scrollTo({ top: list.scrollHeight }));
  });
</script>

<aside class="flex h-full min-h-0 flex-col overflow-hidden text-sm" aria-label="Assistant">
  <div class="flex h-9 shrink-0 items-center gap-1 border-b px-2">
    <span class="flex-1 font-medium">Assistant</span>
    {#if agent.account?.connected}
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label="Undo the assistant’s last change to this page"
        title="Undo the assistant’s last change to this page"
        disabled={!agent.canUndo}
        onclick={() => agent.undo()}
      >
        <Undo2 />
      </Button>
      <Button
        variant={history ? "secondary" : "ghost"}
        size="icon-xs"
        aria-label="Earlier chats"
        aria-pressed={history}
        title="Earlier chats"
        onclick={() => (history = !history)}
      >
        <History />
      </Button>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label="New chat"
        title="New chat"
        onclick={() => {
          history = false;
          agent.newChat();
        }}
      >
        <MessageSquarePlus />
      </Button>
    {/if}
  </div>

  {#if agent.account === null}
    <p class="flex flex-1 items-center justify-center p-4 text-center text-muted-foreground" role="status">
      {agent.failure ?? "Checking the connection…"}
    </p>
  {:else if !agent.account.connected}
    <AgentConnect />
  {:else if history}
    <AgentHistory onopen={() => (history = false)} />
  {:else}
    <div bind:this={list} class="flex min-h-0 flex-1 flex-col gap-5 overflow-x-hidden overflow-y-auto px-5 py-5 leading-relaxed [scrollbar-gutter:stable]" aria-live="polite">
      {#each agent.messages as message, index (index)}
        <div class={message.from === "you" ? "ml-6 flex min-w-0 shrink-0 flex-col gap-2 rounded-lg bg-accent px-4 py-3" : "flex min-w-0 shrink-0 flex-col gap-2"}>
          {#each message.parts as part, position (position)}
            {#if part.kind === "note"}
              <p class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground"><FileText class="size-3 shrink-0" /> <span class="truncate" title={part.text}>{part.text}</span></p>
            {:else if message.from === "you"}
              <p class="min-w-0 wrap-anywhere whitespace-pre-wrap">{part.text}</p>
            {:else if visible(part.text) !== ""}
              <AgentReply text={visible(part.text)} live={agent.running && index === agent.messages.length - 1} />
            {/if}
          {/each}
        </div>
      {:else}
        <div class="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center">
          <Bot class="size-10 text-muted-foreground" aria-hidden="true" />
          <p class="font-medium">What should I write?</p>
          <p class="text-muted-foreground">Try “make a one-page meeting agenda”.</p>
        </div>
      {/each}
      {#if agent.deletion}
        {@const { path, answer } = agent.deletion}
        <div class="flex shrink-0 flex-col gap-3 rounded-lg border p-4" role="alertdialog" aria-label="Confirm deletion">
          <p>The assistant wants to delete “{path}”. It moves to the Recycle Bin, with everything inside it.</p>
          <div class="flex gap-1">
            <Button variant="destructive" size="sm" onclick={() => answer(true)}>Delete</Button>
            <Button variant="outline" size="sm" onclick={() => answer(false)}>Keep</Button>
          </div>
        </div>
      {:else if agent.here}
        <div class="flex min-w-0 shrink-0 flex-col gap-1 text-xs text-muted-foreground" role="status">
          {#each agent.trail as step, position (position)}
            <p class="flex min-w-0 items-center gap-2 {position === agent.trail.length - 1 ? 'text-sm text-foreground' : 'opacity-60'}">
              {#if position === agent.trail.length - 1}<LoaderCircle class="size-4 shrink-0 animate-spin" />{:else}<span class="w-4 shrink-0 text-center">·</span>{/if}
              <span class="truncate">{step}</span>
            </p>
          {/each}
        </div>
      {/if}
      {#if agent.failure}<p class="shrink-0 wrap-anywhere text-destructive" role="alert">{agent.failure}</p>{/if}
    </div>
    <AgentComposer />
  {/if}
</aside>
