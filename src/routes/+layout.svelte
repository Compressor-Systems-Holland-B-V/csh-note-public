<script lang="ts">
  import "../app.css";
  import { loadChats } from "$lib/state/chats.svelte";
  import { connect } from "$lib/state/document.svelte";
  import { settings } from "$lib/state/settings.svelte";
  import { loadThemes, theme } from "$lib/state/theme.svelte";
  import { inDesktopApp, projectNote } from "$lib/ipc";
  import { noteOf, start } from "$lib/state/project.svelte";
  import { setLocale } from "$lib/i18n.svelte";

  let { children } = $props();

  const osPrefersDark = matchMedia("(prefers-color-scheme: dark)").matches;

  $effect(() => {
    const dark = settings.loaded ? settings.values.theme === "dark" : osPrefersDark;
    document.documentElement.classList.toggle("dark", dark);
  });

  $effect(() => {
    const root = document.documentElement;
    const style = theme.style;
    for (const [name, value] of Object.entries(style)) root.style.setProperty(name, value);
    return () => Object.keys(style).forEach((name) => root.style.removeProperty(name));
  });

  $effect(() => {
    const faces = theme.fontFaces.map(({ family, source, weight, style }) => new FontFace(family, source, { weight, style }));
    faces.forEach((face) => document.fonts.add(face));
    return () => faces.forEach((face) => document.fonts.delete(face));
  });

  $effect(() => {
    if (inDesktopApp() && settings.loaded) projectNote(settings.values.uiTheme, noteOf(settings.values)).catch(() => {});
  });

  $effect(() => setLocale(settings.loaded ? settings.values.language : null));

  connect();
  start();
  loadThemes();
  loadChats();
</script>

{@render children()}
