import { provide } from "@zsuite/assistant/host";
import { agent } from "./agent.svelte";
import { chats } from "./chats.svelte";
import { privacy } from "./privacy.svelte";
import { connections } from "./providers.svelte";
import { redaction } from "./redaction.svelte";
import { settings } from "./settings.svelte";
import { theme } from "./theme.svelte";
import { workspace } from "./workspace.svelte";

provide({ agent, chats, privacy, connections, redaction, settings, workspace, theme });
