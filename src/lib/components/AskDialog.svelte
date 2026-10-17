<script lang="ts" module>
  export type { Ask } from "@zsuite/ui/ask-dialog";
</script>

<script lang="ts">
  import { AskDialog, type Ask } from "@zsuite/ui/ask-dialog";
  import { messageOf } from "$lib/utils";
  import { t } from "$lib/i18n.svelte";

  let { ask, onclose }: { ask: Ask; onclose: () => void } = $props();
</script>

<AskDialog {ask} labels={{ cancel: t("common.cancel"), name: t("common.name"), option: t("common.template") }} failed={messageOf} {onclose} />
