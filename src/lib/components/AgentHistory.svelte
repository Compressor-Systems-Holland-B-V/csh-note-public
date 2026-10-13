<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import { Button } from "$lib/components/ui/button";
  import { agent } from "$lib/state/agent.svelte";
  import { chats } from "$lib/state/chats.svelte";

  let { onopen }: { onopen: () => void } = $props();

  let archived = $state(false);
  let ask = $state<Ask | null>(null);

  const shown = $derived(chats.list.filter((chat) => chat.archived === archived));

  function remove(id: string, title: string) {
    ask = {
      title: `Delete the chat “${title}”?`,
      text: "The chat is removed for good. Your pages are not changed.",
      action: "Delete",
      destructive: true,
      run: async () => agent.removeChat(id),
    };
  }
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <div class="flex shrink-0 gap-1 p-2" role="group" aria-label="Which chats to show">
    <Button variant={archived ? "ghost" : "secondary"} size="sm" aria-pressed={!archived} onclick={() => (archived = false)}>Chats</Button>
    <Button variant={archived ? "secondary" : "ghost"} size="sm" aria-pressed={archived} onclick={() => (archived = true)}>Archived</Button>
  </div>
  <ul class="flex min-h-0 flex-1 flex-col gap-1 overflow-auto px-2 pb-2">
    {#each shown as { id, title, updated } (id)}
      <li class="flex min-w-0 shrink-0 items-center gap-1 rounded-md pr-1 hover:bg-accent {id === agent.chatId ? 'bg-accent' : ''}">
        <button
          type="button"
          class="min-w-0 flex-1 rounded-md p-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-current={id === agent.chatId ? "true" : undefined}
          onclick={() => {
            agent.openChat(id);
            onopen();
          }}
        >
          <span class="flex items-center gap-1">
            {#if id === agent.runningChat}<LoaderCircle class="size-3 shrink-0 animate-spin" aria-label="The assistant is working in this chat" />{/if}
            <span class="truncate">{title}</span>
          </span>
          <span class="text-xs text-muted-foreground">{new Date(updated).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}</span>
        </button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={archived ? `Move “${title}” back to chats` : `Archive “${title}”`}
          title={archived ? "Move back to chats" : "Archive"}
          onclick={() => chats.archive(id, !archived)}
        >
          {#if archived}<ArchiveRestore />{:else}<Archive />{/if}
        </Button>
        {#if archived}
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Delete “{title}”"
            title="Delete"
            disabled={id === agent.runningChat}
            onclick={() => remove(id, title)}
          >
            <Trash2 />
          </Button>
        {/if}
      </li>
    {:else}
      <li class="p-2 text-muted-foreground">{archived ? "Nothing is archived." : "No chats yet."}</li>
    {/each}
  </ul>
</div>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
