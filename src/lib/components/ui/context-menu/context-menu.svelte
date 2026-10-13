<script lang="ts">
	import { ContextMenu as ContextMenuPrimitive } from "bits-ui";
	import { i18n } from "$lib/i18n.svelte";

	let { open = $bindable(false), ...restProps }: ContextMenuPrimitive.RootProps = $props();
</script>

<ContextMenuPrimitive.Root bind:open dir={i18n.dir} {...restProps} />
