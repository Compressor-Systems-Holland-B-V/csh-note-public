<script lang="ts" generics="Value">
  import * as Select from "@zsuite/ui/select";

  let {
    value,
    options,
    onchange,
    ...trigger
  }: {
    value: Value;
    options: { value: Value; label: string }[];
    onchange: (value: Value) => void;
    size?: "sm" | "default";
    class?: string;
    id?: string;
    title?: string;
    "aria-label"?: string;
  } = $props();

  const chosen = $derived(options.findIndex((option) => option.value === value));
</script>

<Select.Root type="single" value={String(chosen)} onValueChange={(index) => onchange(options[Number(index)].value)}>
  <Select.Trigger {...trigger}><span class="truncate">{options[chosen]?.label}</span></Select.Trigger>
  <Select.Content>
    {#each options as { label }, index (index)}
      <Select.Item value={String(index)} {label}>{label}</Select.Item>
    {/each}
  </Select.Content>
</Select.Root>
