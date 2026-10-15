<script lang="ts">
  import { Button } from "@zsuite/ui/button";
  import * as Dialog from "@zsuite/ui/dialog";
  import { projects } from "$lib/state/project.svelte";
  import { t } from "$lib/i18n.svelte";

  const fonts = $derived(projects.missingFonts.length + projects.leftOutFonts.length > 0);

  function seen() {
    projects.missingFonts = [];
    projects.leftOutFonts = [];
  }
</script>

{#snippet list(names: string[])}
  <ul class="list-disc ps-5 text-sm">
    {#each names as name (name)}<li>{name}</li>{/each}
  </ul>
{/snippet}

{#if projects.offer}
  <Dialog.Root open onOpenChange={(open) => open || projects.decline()}>
    <Dialog.Content>
      <Dialog.Header>
        <Dialog.Title>{t("workspace.file.themesTitle")}</Dialog.Title>
        <Dialog.Description>{t("workspace.file.themesText")}</Dialog.Description>
      </Dialog.Header>
      {@render list(projects.offer.themes)}
      <Dialog.Footer>
        <Button variant="outline" onclick={() => projects.decline()}>{t("common.cancel")}</Button>
        <Button onclick={() => projects.accept()}>{t("workspace.file.themesInstall")}</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{:else if fonts && !projects.choosing}
  <Dialog.Root open onOpenChange={(open) => open || seen()}>
    <Dialog.Content>
      <Dialog.Header>
        <Dialog.Title>{t("workspace.file.fontsTitle")}</Dialog.Title>
        <Dialog.Description>{t("workspace.file.fontsFallback")}</Dialog.Description>
      </Dialog.Header>
      {#if projects.missingFonts.length > 0}
        <p class="text-sm">{t("workspace.file.fontsMissing")}</p>
        {@render list(projects.missingFonts)}
      {/if}
      {#if projects.leftOutFonts.length > 0}
        <p class="text-sm">{t("workspace.file.fontsLeftOut")}</p>
        {@render list(projects.leftOutFonts)}
      {/if}
      <Dialog.Footer>
        <Button onclick={seen}>{t("workspace.file.fontsSeen")}</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{/if}
