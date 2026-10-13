<script lang="ts">
  import "../app.css";
  import { connect } from "$lib/state/document.svelte";
  import { theme } from "$lib/state/theme.svelte";

  let { children } = $props();

  theme.load();
  connect();
</script>

{@render children()}
