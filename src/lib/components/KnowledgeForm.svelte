<script lang="ts" module>
  export type FormMode = "create" | "import" | "unlock" | "export" | "password";
</script>

<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { privacy } from "$lib/state/privacy.svelte";
  import { messageOf } from "$lib/utils";
  import { t } from "$lib/i18n.svelte";

  let { mode, ondone, oncancel }: { mode: FormMode; ondone: () => void; oncancel?: () => void } = $props();

  const minimum = 8;

  let name = $state(t("privacy.form.defaultName"));
  let current = $state("");
  let password = $state("");
  let again = $state("");
  let remember = $state(true);
  let picked = $state<string | null>(null);
  let failure = $state<string | null>(null);

  const fresh = $derived(mode === "create" || mode === "export" || mode === "password");
  const mismatch = $derived(fresh && again !== "" && again !== password);
  const short = $derived(fresh && password.length > 0 && password.length < minimum);
  const ready = $derived(password !== "" && !mismatch && !short && (!fresh || again === password) && (mode !== "import" || picked !== null) && (mode !== "password" || current !== ""));

  async function choose() {
    failure = null;
    try {
      picked = (await privacy.pick()) ?? picked;
    } catch (error) {
      failure = messageOf(error);
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!ready) return;
    failure = null;
    try {
      if (mode === "create") await privacy.create(name, password, remember);
      else if (mode === "import") await privacy.import(password, remember);
      else if (mode === "unlock") await privacy.unlock(password, remember);
      else if (mode === "export") {
        if (!(await privacy.export(password))) return;
      } else await privacy.changePassword(current, password, remember);
      password = again = current = "";
      ondone();
    } catch (error) {
      failure = messageOf(error);
    }
  }
</script>

<form class="grid gap-3" onsubmit={submit}>
  {#if mode === "create"}
    <div class="grid gap-1.5">
      <Label for="knowledge-name">{t("privacy.form.name")}</Label>
      <Input id="knowledge-name" bind:value={name} autocomplete="off" />
    </div>
  {:else if mode === "import"}
    <div class="flex items-center gap-2">
      <Button type="button" variant="outline" size="sm" onclick={choose}>{t("privacy.form.choose")}</Button>
      <span class="min-w-0 flex-1 truncate text-sm text-muted-foreground" title={picked ?? ""}>{picked ?? t("privacy.form.nothingChosen")}</span>
    </div>
  {:else if mode === "password"}
    <div class="grid gap-1.5">
      <Label for="knowledge-current">{t("privacy.form.current")}</Label>
      <Input id="knowledge-current" type="password" bind:value={current} autocomplete="current-password" />
    </div>
  {/if}
  <div class="grid gap-1.5">
    <Label for="knowledge-password">{t(fresh ? "privacy.form.newPassword" : "privacy.form.password")}</Label>
    <Input id="knowledge-password" type="password" bind:value={password} autocomplete={fresh ? "new-password" : "current-password"} aria-invalid={short || failure !== null} />
    {#if short}<p class="text-xs text-destructive">{t("privacy.form.tooShort", { count: minimum })}</p>{/if}
  </div>
  {#if fresh}
    <div class="grid gap-1.5">
      <Label for="knowledge-again">{t("privacy.form.again")}</Label>
      <Input id="knowledge-again" type="password" bind:value={again} autocomplete="new-password" aria-invalid={mismatch} />
      {#if mismatch}<p class="text-xs text-destructive">{t("privacy.form.mismatch")}</p>{/if}
    </div>
  {/if}
  {#if mode === "export"}
    <p class="text-xs text-muted-foreground">{t("privacy.form.exportHint")}</p>
  {:else}
    <div class="flex items-center justify-between gap-3">
      <div>
        <Label for="knowledge-remember">{t("privacy.form.remember")}</Label>
        <p class="text-xs text-muted-foreground">{t("privacy.form.rememberHint")}</p>
      </div>
      <Switch id="knowledge-remember" bind:checked={remember} />
    </div>
  {/if}
  {#if failure}<p class="text-sm wrap-anywhere text-destructive" role="alert">{failure}</p>{/if}
  <div class="flex justify-end gap-2">
    {#if oncancel}<Button type="button" variant="ghost" size="sm" onclick={oncancel}>{t("common.cancel")}</Button>{/if}
    <Button type="submit" size="sm" disabled={!ready || privacy.busy}>{t(`privacy.form.submit.${mode}`)}</Button>
  </div>
</form>
