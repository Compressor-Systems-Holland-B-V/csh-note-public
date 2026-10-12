<script lang="ts">
  import "../app.css";
  import { theme } from "$lib/state/theme.svelte";

  let { children } = $props();

  theme.load();
</script>

{@render children()}
