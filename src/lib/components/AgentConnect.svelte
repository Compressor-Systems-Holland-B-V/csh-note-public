<script lang="ts">
  import Bot from "@lucide/svelte/icons/bot";
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import UserRound from "@lucide/svelte/icons/user-round";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { agent } from "$lib/state/agent.svelte";

  let code = $state("");
  let copied = $state(false);

  async function copy(link: string) {
    await navigator.clipboard.writeText(link);
    copied = true;
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    agent.sendCode(code);
    code = "";
  }

  const ways = [
    { method: null, label: "Claude subscription", hint: "Pro, Max or Team plan", icon: UserRound },
    { method: "console", label: "API account", hint: "Pay per use", icon: KeyRound },
    { method: "sso", label: "Single sign-on", hint: "Through your company", icon: ShieldCheck },
  ];
</script>

<div class="flex flex-1 flex-col items-center justify-center gap-3 p-4 text-center">
  <Bot class="size-10 text-muted-foreground" aria-hidden="true" />
  <p class="font-medium">Connect the assistant</p>
  <p class="text-muted-foreground">It writes and edits your pages for you. Sign in once in your browser.</p>
  {#if agent.stage}
    <p class="flex items-center gap-2" role="status">
      <LoaderCircle class="size-4 animate-spin" />
      {agent.stage === "installing" ? "Downloading Claude Code…" : "Finish signing in in your browser…"}
    </p>
    {#if agent.stage === "connecting" && agent.link}
      {@const link = agent.link}
      <div class="grid w-full max-w-64 gap-2 border-t pt-3 text-left">
        <p class="text-xs text-muted-foreground">
          Browser did not open? Copy this link, open it in any browser, then paste the code it shows you.
        </p>
        <div class="flex gap-1">
          <Input readonly value={link} aria-label="Sign-in link" class="h-8 text-xs" onfocus={(event) => event.currentTarget.select()} />
          <Button variant="outline" size="icon-sm" aria-label="Copy the sign-in link" title="Copy the sign-in link" onclick={() => copy(link)}>
            {#if copied}<Check />{:else}<Copy />{/if}
          </Button>
        </div>
        <form class="flex gap-1" onsubmit={submit}>
          <Input bind:value={code} aria-label="Code from the browser" placeholder="Code from the browser" class="h-8 text-xs" autocomplete="off" />
          <Button type="submit" size="sm" disabled={code.trim() === ""}>Connect</Button>
        </form>
      </div>
    {/if}
    <Button variant="outline" size="sm" onclick={() => agent.stop()}>Cancel</Button>
  {:else}
    <div class="grid w-full max-w-64 gap-2">
      {#each ways as { method, label, hint, icon: Icon }, index (label)}
        <Button
          variant={index === 0 ? "default" : "outline"}
          class="h-auto justify-start gap-3 py-2 text-left whitespace-normal"
          onclick={() => agent.connect(method)}
        >
          <Icon class="size-5" />
          <span class="grid">
            <span>{label}</span>
            <span class="text-xs font-normal opacity-70">{hint}</span>
          </span>
        </Button>
      {/each}
    </div>
  {/if}
  {#if agent.failure}<p class="text-destructive" role="alert">{agent.failure}</p>{/if}
  <p class="text-xs text-muted-foreground">Runs Claude Code with your own account.</p>
</div>
