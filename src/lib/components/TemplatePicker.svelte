<script lang="ts">
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import { Button } from "$lib/components/ui/button";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { doc } from "$lib/state/document.svelte";
  import { templateOf, theme, withTemplate } from "$lib/state/theme.svelte";
  import { workspace } from "$lib/state/workspace.svelte";
  import { t } from "$lib/i18n.svelte";
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="sm" class="max-w-48 min-w-0" title={t("editor.templateTitle")}>
        <LayoutTemplate /> <span class="truncate">{templateOf(doc.text, theme.templates)?.label ?? t("common.template")}</span>
      </Button>
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
