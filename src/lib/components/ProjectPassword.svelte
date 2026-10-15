<script lang="ts">
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import { Button } from "@zsuite/ui/button";
  import * as Dialog from "@zsuite/ui/dialog";
  import { Input } from "@zsuite/ui/input";
  import { Label } from "@zsuite/ui/label";
  import { projects, titleOf } from "$lib/state/project.svelte";
  import { t } from "$lib/i18n.svelte";

  const minimum = 8;

  let password = $state("");
  let again = $state("");

  const fresh = $derived(projects.creating !== null);
  const short = $derived(fresh && password.length > 0 && password.length < minimum);
  const mismatch = $derived(fresh && again !== "" && again !== password);
  const ready = $derived(password !== "" && !short && !mismatch && (!fresh || again === password));

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!ready || projects.busy) return;
    const typed = password;
    password = again = "";
    if (fresh) await projects.createWith(typed);
    else await projects.unlock(typed);
  }
</script>

<Dialog.Root open onOpenChange={(open) => open || projects.cancelPassword()}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <LockKeyhole class="size-5" />
        {fresh ? t("workspace.password.newTitle") : t("workspace.password.unlockTitle", { name: titleOf(projects.locked ?? "") })}
      </Dialog.Title>
      <Dialog.Description>{t(fresh ? "workspace.password.newText" : "workspace.password.unlockText")}</Dialog.Description>
    </Dialog.Header>
    <form class="grid gap-3" onsubmit={submit}>
      <div class="grid gap-1.5">
        <Label for="project-password">{t("workspace.password.password")}</Label>
        <Input id="project-password" type="password" bind:value={password} autocomplete={fresh ? "new-password" : "current-password"} aria-invalid={short || projects.failure !== null} />
        {#if short}<p class="text-xs text-destructive">{t("privacy.form.tooShort", { count: minimum })}</p>{/if}
      </div>
      {#if fresh}
        <div class="grid gap-1.5">
          <Label for="project-again">{t("privacy.form.again")}</Label>
          <Input id="project-again" type="password" bind:value={again} autocomplete="new-password" aria-invalid={mismatch} />
          {#if mismatch}<p class="text-xs text-destructive">{t("privacy.form.mismatch")}</p>{/if}
        </div>
        <p class="text-xs text-muted-foreground">{t("workspace.password.forgotten")}</p>
      {/if}
      {#if projects.failure}<p class="text-sm wrap-anywhere text-destructive" role="alert">{projects.failure}</p>{/if}
      <div class="flex justify-end gap-2">
        <Button type="button" variant="ghost" size="sm" onclick={() => projects.cancelPassword()}>{t("common.cancel")}</Button>
        <Button type="submit" size="sm" disabled={!ready || projects.busy}>{t(fresh ? "workspace.password.create" : "workspace.password.open")}</Button>
      </div>
    </form>
  </Dialog.Content>
</Dialog.Root>
