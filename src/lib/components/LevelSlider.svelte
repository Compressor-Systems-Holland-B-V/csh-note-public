<script lang="ts">
  import { privacy } from "$lib/state/privacy.svelte";
  import { t } from "$lib/i18n.svelte";

  let { level, onlevel, class: className = "" }: { level: number; onlevel: (level: number) => void; class?: string } = $props();

  const levels = ["own", "light", "medium", "strict"];
</script>

<label class="grid gap-1 text-sm {className}">
  <span class="flex justify-between gap-2"><span class="font-medium">{t("privacy.level.label")}</span><span class="text-muted-foreground">{t(`privacy.level.${levels[level]}`)}</span></span>
  <input type="range" min={privacy.policy?.minimumLevel ?? 0} max={levels.length - 1} step="1" value={level} class="accent-primary" oninput={(event) => onlevel(Number(event.currentTarget.value))} />
</label>
