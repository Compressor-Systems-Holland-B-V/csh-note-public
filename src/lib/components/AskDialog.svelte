<script lang="ts" module>
  export type Ask = {
    title: string;
    text?: string;
    value?: string;
    action: string;
    destructive?: boolean;
    options?: string[];
    run: (value: string, option: number) => Promise<void>;
  };
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { messageOf } from "$lib/utils";
  import Picker from "./Picker.svelte";

  let { ask, onclose }: { ask: Ask; onclose: () => void } = $props();

  let value = $state(untrack(() => ask.value ?? ""));
  let option = $state(0);
  let failure = $state<string | null>(null);
  let busy = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    try {
      await ask.run(value, option);
      onclose();
    } catch (error) {
      failure = messageOf(error);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root
  open
  onOpenChange={(open) => {
    if (!open) onclose();
  }}
>
  <Dialog.Content showCloseButton={false}>
    <form class="grid gap-4" onsubmit={submit}>
      <Dialog.Header>
        <Dialog.Title>{ask.title}</Dialog.Title>
        {#if ask.text}<Dialog.Description>{ask.text}</Dialog.Description>{/if}
      </Dialog.Header>
      {#if ask.value !== undefined}
        <Input bind:value aria-label="Name" aria-invalid={failure !== null} autocomplete="off" />
      {/if}
      {#if ask.options}
        <Picker
          aria-label="Template"
          class="w-full"
          value={option}
          options={ask.options.map((label, value) => ({ value, label }))}
          onchange={(chosen) => (option = chosen)}
        />
      {/if}
      {#if failure}<p class="text-sm whitespace-pre-line text-destructive" role="alert">{failure}</p>{/if}
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={onclose}>Cancel</Button>
        <Button type="submit" variant={ask.destructive ? "destructive" : "default"} disabled={busy}>
          {ask.action}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
