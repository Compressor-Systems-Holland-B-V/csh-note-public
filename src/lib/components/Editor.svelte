<script lang="ts">
  import { untrack } from "svelte";
  import { minimalSetup } from "codemirror";
  import { HighlightStyle, LanguageSupport, syntaxHighlighting } from "@codemirror/language";
  import { setDiagnostics } from "@codemirror/lint";
  import { getOriginalDoc, unifiedMergeView } from "@codemirror/merge";
  import { Compartment, EditorState } from "@codemirror/state";
  import { Decoration, EditorView, ViewPlugin, lineNumbers } from "@codemirror/view";
  import type { DecorationSet, ViewUpdate } from "@codemirror/view";
  import { tags } from "@lezer/highlight";
  import { typst_lezer } from "codemirror-lang-typst/lezer";
  import type { Diagnostic } from "$lib/ipc";
  import { t } from "$lib/i18n.svelte";

  let {
    text,
    diagnostics,
    fontSize,
    wordWrap,
    locked = false,
    highlight = null,
    original = null,
    onchange,
    onoriginal,
  }: {
    text: string;
    diagnostics: Diagnostic[];
    fontSize: number;
    wordWrap: boolean;
    locked?: boolean;
    highlight?: { from: number; to: number } | null;
    original?: string | null;
    onchange: (text: string) => void;
    onoriginal?: (text: string) => void;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;

  const look = new Compartment();
  const lock = new Compartment();
  const marks = new Compartment();
  const review = new Compartment();
  let compared: string | null = null;

  function control(kind: "accept" | "reject", action: (event: MouseEvent) => void) {
    const button = document.createElement("button");
    button.type = "button";
    button.name = kind;
    button.textContent = t(kind === "accept" ? "editor.review.keep" : "editor.review.undo");
    button.title = t(kind === "accept" ? "editor.review.keepTitle" : "editor.review.undoTitle");
    button.onmousedown = action;
    return button;
  }
  const written = Decoration.line({ class: "cm-written" });

  const eitherWay = Decoration.line({ attributes: { dir: "auto" } });
  const lineDirections = (view: EditorView) =>
    Decoration.set(view.viewportLineBlocks.map((block) => eitherWay.range(block.from)));
  const bothDirections = [
    EditorView.perLineTextDirection.of(true),
    ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
          this.decorations = lineDirections(view);
        }
        update(update: ViewUpdate) {
          if (update.docChanged || update.viewportChanged) this.decorations = lineDirections(update.view);
        }
      },
      { decorations: (plugin) => plugin.decorations },
    ),
  ];

  const colours = EditorView.theme({
    "&": { height: "100%", color: "var(--foreground)", backgroundColor: "var(--background)" },
    "&.cm-focused": { outline: "2px solid var(--ring)", outlineOffset: "-2px" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
    ".cm-content": { caretColor: "var(--foreground)" },
    ".cm-cursor": { borderLeftColor: "var(--foreground)" },
    ".cm-gutters": {
      color: "var(--muted-foreground)",
      backgroundColor: "var(--background)",
      borderRight: "1px solid var(--border)",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "var(--accent)",
    },
    ".cm-tooltip": {
      color: "var(--popover-foreground)",
      backgroundColor: "var(--popover)",
      border: "1px solid var(--border)",
    },
    ".cm-written": { backgroundColor: "color-mix(in oklab, var(--chart-2) 25%, transparent)" },
    "&.cm-merge-b .cm-changedLine, .cm-inlineChangedLine": { backgroundColor: "color-mix(in oklab, var(--chart-2) 15%, transparent)" },
    "&.cm-merge-b .cm-changedText": { background: "color-mix(in oklab, var(--chart-2) 35%, transparent)" },
    ".cm-deletedChunk": { backgroundColor: "color-mix(in oklab, var(--destructive) 12%, transparent)" },
    "& .cm-deletedChunk .cm-deletedText": { background: "color-mix(in oklab, var(--destructive) 30%, transparent)" },
    "&.cm-merge-b .cm-changedLineGutter": { background: "var(--chart-2)" },
    "& .cm-deletedLineGutter": { background: "var(--destructive)" },
    ".cm-deletedChunk .cm-chunkButtons button": {
      font: "inherit",
      fontFamily: "var(--font-sans)",
      padding: "0 0.5rem",
      marginLeft: "0.25rem",
      borderRadius: "calc(var(--radius) - 4px)",
      border: "1px solid var(--border)",
      cursor: "pointer",
    },
    ".cm-deletedChunk .cm-chunkButtons button[name=accept]": { color: "var(--primary-foreground)", background: "var(--primary)" },
    ".cm-deletedChunk .cm-chunkButtons button[name=reject]": { color: "var(--foreground)", background: "var(--background)" },
    ".cm-diagnostic-error": { borderLeftColor: "var(--destructive)" },
    ".cm-diagnostic-warning": { borderLeftColor: "var(--chart-4)" },
    ".cm-lintRange-error": {
      backgroundImage: "none",
      textDecoration: "underline wavy var(--destructive)",
    },
    ".cm-lintRange-warning": {
      backgroundImage: "none",
      textDecoration: "underline wavy var(--chart-4)",
    },
  });

  const highlighting = HighlightStyle.define([
    { tag: tags.comment, color: "var(--muted-foreground)", fontStyle: "italic" },
    { tag: tags.heading, fontWeight: "bold", color: "var(--chart-3)" },
    { tag: tags.strong, fontWeight: "bold" },
    { tag: tags.emphasis, fontStyle: "italic" },
    { tag: tags.link, textDecoration: "underline" },
    { tag: [tags.keyword, tags.bool], color: "var(--chart-1)" },
    { tag: [tags.string, tags.monospace], color: "var(--chart-2)" },
    { tag: tags.number, color: "var(--chart-5)" },
    { tag: [tags.function(tags.variableName), tags.propertyName], color: "var(--chart-3)" },
    { tag: tags.escape, color: "var(--chart-5)" },
    { tag: [tags.punctuation, tags.operator, tags.paren], color: "var(--muted-foreground)" },
    { tag: tags.invalid, color: "var(--destructive)" },
  ]);

  $effect(() => {
    const created = new EditorView({
      parent: host,
      doc: untrack(() => text),
      extensions: [
        minimalSetup,
        lineNumbers(),
        bothDirections,
        look.of([]),
        lock.of([]),
        marks.of([]),
        review.of([]),
        new LanguageSupport(typst_lezer().language),
        syntaxHighlighting(highlighting),
        colours,
        EditorView.contentAttributes.of({ "aria-label": t("editor.source") }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onchange(update.state.doc.toString());
          if (compared === null) return;
          const now = getOriginalDoc(update.state).toString();
          if (now === compared) return;
          compared = now;
          onoriginal?.(now);
        }),
      ],
    });
    view = created;
    return () => created.destroy();
  });

  $effect(() => {
    view?.dispatch({
      effects: look.reconfigure([
        wordWrap ? EditorView.lineWrapping : [],
        EditorView.theme({ "&": { fontSize: `${fontSize}px` } }),
      ]),
    });
  });

  $effect(() => {
    view?.dispatch({ effects: lock.reconfigure([EditorState.readOnly.of(locked), EditorView.editable.of(!locked)]) });
  });

  $effect(() => {
    const text = view?.state.doc;
    if (!view || !text) return;
    const numbers = highlight ? Array.from({ length: highlight.to - highlight.from + 1 }, (_, line) => highlight.from + line) : [];
    const starts = numbers.filter((line) => line >= 1 && line <= text.lines).map((line) => text.line(line).from);
    view.dispatch({
      effects: [
        marks.reconfigure(EditorView.decorations.of(Decoration.set(starts.map((start) => written.range(start))))),
        ...starts.slice(0, 1).map((start) => EditorView.scrollIntoView(start, { y: "center" })),
      ],
    });
  });

  $effect(() => {
    if (!view || original === compared) return;
    compared = original;
    view.dispatch({
      effects: review.reconfigure(original === null ? [] : unifiedMergeView({ original, mergeControls: control, gutter: true })),
    });
  });

  $effect(() => {
    if (!view) return;
    const length = view.state.doc.length;
    view.dispatch(
      setDiagnostics(
        view.state,
        diagnostics.map(({ severity, message, from, to }) => ({
          severity,
          message,
          from: Math.min(from, length),
          to: Math.min(Math.max(to, from), length),
        })),
      ),
    );
  });
</script>

<div bind:this={host} dir="ltr" class="h-full overflow-hidden"></div>
