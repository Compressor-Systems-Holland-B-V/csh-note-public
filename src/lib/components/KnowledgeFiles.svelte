<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import AskDialog, { type Ask } from "$lib/components/AskDialog.svelte";
  import { Button } from "@zsuite/ui/button";
  import { privacy } from "$lib/state/privacy.svelte";
  import KnowledgeForm from "./KnowledgeForm.svelte";
  import type { FormMode } from "./KnowledgeForm.svelte";
  import { t } from "$lib/i18n.svelte";

  let form = $state<FormMode | null>(null);
  let ask = $state<Ask | null>(null);
  let exported = $state(false);

  const files = $derived(privacy.status?.files ?? []);
  const active = $derived(privacy.status?.active ?? null);

  function remove(id: string, name: string) {
    ask = {
      title: t("privacy.files.removeTitle", { name }),
      text: t("privacy.files.removeText"),
      action: t("common.delete"),
      destructive: true,
      run: async () => void (await privacy.remove(id)),
    };
  }
</script>

<section class="grid gap-2" aria-labelledby="knowledge-files">
  <h3 id="knowledge-files" class="text-sm font-medium">{t("privacy.files.title")}</h3>
  {#if files.length === 0}
    <p class="text-xs text-muted-foreground">{t("privacy.files.none")}</p>
  {/if}
  <ul class="grid gap-1">
    {#each files as { id, name } (id)}
      <li class="flex items-center gap-2 rounded-md border px-2 py-1.5 {id === active ? 'border-ring' : ''}">
        {#if id === active}<Check class="size-4 shrink-0" aria-label={t("privacy.files.active")} />{/if}
        <span class="min-w-0 flex-1 truncate text-sm" title={name}>{name}</span>
        {#if id === active}
          <span class="text-xs text-muted-foreground">{t(privacy.open ? "privacy.files.open" : "privacy.files.locked")}</span>
        {:else}
          <Button variant="outline" size="xs" onclick={() => privacy.activate(id)}>{t("privacy.files.use")}</Button>
        {/if}
        <Button variant="ghost" size="xs" class="text-destructive" onclick={() => remove(id, name)}>{t("common.delete")}</Button>
      </li>
    {/each}
  </ul>
  <div class="flex flex-wrap gap-1">
    <Button variant="outline" size="sm" onclick={() => (form = "create")}>{t("privacy.files.new")}</Button>
    <Button variant="outline" size="sm" onclick={() => (form = "import")}>{t("privacy.files.import")}</Button>
    {#if privacy.locked}
      <Button size="sm" onclick={() => (form = "unlock")}>{t("privacy.files.unlock")}</Button>
    {:else if privacy.open}
      <Button variant="outline" size="sm" onclick={() => (form = "export")}>{t("privacy.files.export")}</Button>
      <Button variant="outline" size="sm" onclick={() => (form = "password")}>{t("privacy.files.password")}</Button>
    {/if}
  </div>
  {#if exported}<p class="text-xs text-muted-foreground" role="status">{t("privacy.files.exported")}</p>{/if}
  {#if form}
    <div class="rounded-md border p-3">
      <p class="mb-2 text-sm font-medium">{t(`privacy.form.title.${form}`)}</p>
      <KnowledgeForm
        mode={form}
        ondone={() => {
          exported = form === "export";
          form = null;
        }}
        oncancel={() => (form = null)}
      />
    </div>
  {/if}
</section>

{#if ask}<AskDialog {ask} onclose={() => (ask = null)} />{/if}
