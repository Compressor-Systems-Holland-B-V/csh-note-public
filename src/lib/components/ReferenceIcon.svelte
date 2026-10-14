<script lang="ts">
  import Book from "@lucide/svelte/icons/book";
  import File from "@lucide/svelte/icons/file";
  import FileCode from "@lucide/svelte/icons/file-code";
  import FileImage from "@lucide/svelte/icons/file-image";
  import FileSpreadsheet from "@lucide/svelte/icons/file-spreadsheet";
  import FileText from "@lucide/svelte/icons/file-text";
  import FileType from "@lucide/svelte/icons/file-type";
  import Folder from "@lucide/svelte/icons/folder";
  import Presentation from "@lucide/svelte/icons/presentation";
  import { referenceKind, type ReferenceKind } from "$lib/assistant";

  let { label, class: className = "size-3 shrink-0" }: { label: string; class?: string } = $props();

  const icons: Record<ReferenceKind, typeof File> = {
    project: Folder,
    notebook: Book,
    page: FileText,
    image: FileImage,
    sheet: FileSpreadsheet,
    slides: Presentation,
    document: FileType,
    code: FileCode,
    file: File,
  };

  const Icon = $derived(icons[referenceKind(label)]);
</script>

<Icon class={className} aria-hidden="true" />
