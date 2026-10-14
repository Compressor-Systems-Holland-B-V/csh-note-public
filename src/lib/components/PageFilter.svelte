<script lang="ts">
  import { privacy } from "$lib/state/privacy.svelte";
  import LevelSlider from "./LevelSlider.svelte";
  import PrivacyView from "./PrivacyView.svelte";

  let { page, text }: { page: string; text: string } = $props();
</script>

<div class="flex h-full flex-col">
  <LevelSlider class="shrink-0 border-b px-3 py-2" level={privacy.pageLevel(page)} onlevel={(level) => privacy.setPageLevel(page, level)} />
  <div class="min-h-0 flex-1"><PrivacyView {text} level={privacy.pageLevel(page)} /></div>
</div>
