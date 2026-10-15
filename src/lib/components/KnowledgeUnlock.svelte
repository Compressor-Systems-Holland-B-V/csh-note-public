<script lang="ts">
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import * as Dialog from "@zsuite/ui/dialog";
  import { privacy } from "$lib/state/privacy.svelte";
  import KnowledgeForm from "./KnowledgeForm.svelte";
  import { t } from "$lib/i18n.svelte";

  const close = () => (privacy.unlockDismissed = true);
</script>

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) close();
  }}
>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2"><LockKeyhole class="size-5" /> {t("privacy.unlock.title")}</Dialog.Title>
      <Dialog.Description>{t("privacy.unlock.text", { name: privacy.activeName ?? "" })}</Dialog.Description>
    </Dialog.Header>
    <KnowledgeForm mode="unlock" ondone={close} oncancel={close} />
  </Dialog.Content>
</Dialog.Root>
