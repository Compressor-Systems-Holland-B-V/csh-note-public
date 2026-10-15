<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import { Button } from "@zsuite/ui/button";
  import { agent } from "$lib/state/agent.svelte";
  import { chats } from "$lib/state/chats.svelte";
  import { i18n, t } from "$lib/i18n.svelte";

  let { onopen }: { onopen: () => void } = $props();

  let archived = $state(false);
  let ask = $state<Ask | null>(null);

  const shown = $derived(chats.list.filter((chat) => chat.archived === archived));

  function remove(id: string, title: string) {
    ask = {
      title: t("assistant.history.deleteTitle", { title }),
      text: t("assistant.history.deleteText"),
      action: t("common.delete"),
      destructive: true,
      run: async () => agent.removeChat(id),
    };
  }

  function all() {
    const ids = shown.map(({ id }) => id);
    ask = archived
      ? {
          title: t("assistant.history.deleteAllTitle"),
          text: t("assistant.history.deleteAllText"),
          action: t("assistant.history.deleteAll"),
          destructive: true,
          run: async () => ids.forEach((id) => agent.removeChat(id)),
        }
      : {
          title: t("assistant.history.archiveAllTitle"),
          action: t("assistant.history.archiveAll"),
          run: async () => ids.forEach((id) => chats.archive(id, true)),
        };
  }
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <div class="flex shrink-0 flex-wrap items-center gap-1 p-2">
    <Button variant="ghost" size="icon-sm" aria-label={t("assistant.history.back")} title={t("assistant.history.back")} onclick={onopen}>
      <ArrowLeft class="rtl:rotate-180" />
    </Button>
    <div class="flex gap-1" role="group" aria-label={t("assistant.history.which")}>
      <Button variant={archived ? "ghost" : "secondary"} size="sm" aria-pressed={!archived} onclick={() => (archived = false)}>{t("assistant.history.chats")}</Button>
      <Button variant={archived ? "secondary" : "ghost"} size="sm" aria-pressed={archived} onclick={() => (archived = true)}>{t("assistant.history.archived")}</Button>
    </div>
    <Button variant="ghost" size="sm" class="ms-auto" disabled={shown.length === 0} onclick={all}>
      {#if archived}<Trash2 />{:else}<Archive />{/if}
      {t(archived ? "assistant.history.deleteAll" : "assistant.history.archiveAll")}
    </Button>
  </div>
  <ul class="flex min-h-0 flex-1 flex-col gap-1 overflow-auto px-2 pb-2">
    {#each shown as { id, title, updated } (id)}
      <li class="flex min-w-0 shrink-0 items-center gap-1 rounded-md pe-1 hover:bg-accent {id === agent.chatId ? 'bg-accent' : ''}">
        <button
          type="button"
          class="min-w-0 flex-1 rounded-md p-2 text-start outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-current={id === agent.chatId ? "true" : undefined}
          onclick={() => {
            agent.openChat(id);
            onopen();
          }}
        >
          <span class="flex items-center gap-1">
            {#if id === agent.runningChat}<LoaderCircle class="size-3 shrink-0 animate-spin" aria-label={t("assistant.history.working")} />{/if}
            <span class="truncate">{title}</span>
          </span>
          <span class="text-xs text-muted-foreground">{new Date(updated).toLocaleString(i18n.locale, { dateStyle: "medium", timeStyle: "short" })}</span>
        </button>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={t(archived ? "assistant.history.restoreChat" : "assistant.history.archiveChat", { title })}
          title={t(archived ? "assistant.history.restore" : "assistant.history.archive")}
          onclick={() => chats.archive(id, !archived)}
        >
          {#if archived}<ArchiveRestore />{:else}<Archive />{/if}
        </Button>
        {#if archived}
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={t("assistant.history.deleteChat", { title })}
            title={t("common.delete")}
            disabled={id === agent.runningChat}
            onclick={() => remove(id, title)}
          >
            <Trash2 />
          </Button>
        {/if}
      </li>
    {:else}
      <li class="p-2 text-muted-foreground">{t(archived ? "assistant.history.noneArchived" : "assistant.history.none")}</li>
    {/each}
  </ul>
</div>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
