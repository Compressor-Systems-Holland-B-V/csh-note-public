<script lang="ts">
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import { Button } from "@zsuite/ui/button";
  import * as DropdownMenu from "@zsuite/ui/dropdown-menu";
  import { doc } from "$lib/state/document.svelte";
  import { templateOf, theme, withTemplate } from "$lib/state/theme.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  const label = $derived(`${t("editor.templateTitle")}: ${templateOf(doc.text, theme.templates)?.label ?? t("common.template")}`);
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" aria-label={label} title={label}><LayoutTemplate /></Button>
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Content align="start">
    {#each theme.offered as template (template.id)}
      <DropdownMenu.Item onSelect={() => workspace.rewrite((text) => withTemplate(text, template))}>
        {template.label}
      </DropdownMenu.Item>
    {/each}
  </DropdownMenu.Content>
</DropdownMenu.Root>
