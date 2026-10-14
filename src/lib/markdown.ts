import DOMPurify from "dompurify";
import { marked } from "marked";
import type { Piece } from "$lib/ipc";

const allowed = {
  ALLOWED_TAGS: ["p", "br", "strong", "em", "del", "code", "pre", "ul", "ol", "li", "blockquote"],
  ALLOWED_ATTR: [],
};

export const longReply = (text: string) => text.length > 600 || text.split("\n").length > 12;

export function render(text: string, purify: Pick<typeof DOMPurify, "sanitize"> = DOMPurify) {
  return purify.sanitize(marked.parse(text, { async: false, breaks: true }), allowed);
}

export type Mark = { text: string; title: string };

const escaped = (text: string) => text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const asHtml = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const literal = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export function withMarks(html: string, restored: Mark[], unknown: string) {
  const texts = [...new Map(restored.filter(({ text }) => text.trim() !== "").map((mark) => [asHtml(mark.text), mark.title])).entries()].sort(
    ([first], [second]) => second.length - first.length,
  );
  const shapes = [...texts.map(([text]) => literal(text)), "⟦[^⟦⟧\n<>]{1,38}⟧"];
  const pattern = new RegExp(shapes.join("|"), "g");
  const titles = new Map(texts);
  return html
    .split(/(<[^>]*>)/)
    .map((piece) =>
      piece.startsWith("<")
        ? piece
        : piece.replace(pattern, (found) => {
            const title = titles.get(found);
            return title === undefined
              ? `<span class="text-destructive" title="${escaped(unknown)}">${found}</span>`
              : `<span class="underline decoration-dotted underline-offset-2" title="${escaped(title)}">${found}</span>`;
          }),
    )
    .join("");
}

const documentAllowed = {
  ALLOWED_TAGS: [...allowed.ALLOWED_TAGS, "h1", "h2", "h3", "h4", "h5", "h6", "hr", "table", "thead", "tbody", "tr", "th", "td"],
  ALLOWED_ATTR: [],
};

export const structured = (path?: string) => path !== undefined && /\.(docx|pptx|xlsx|odt|odp|ods)\.txt$|\.(pdf|md)$/i.test(path);

export type TokenLook = { className: string; title: string };

export function piecesHtml(pieces: Piece[], look: (piece: Piece) => TokenLook, purify: Pick<typeof DOMPurify, "sanitize"> = DOMPurify) {
  const source = pieces.map((piece, index) => (piece.token ? `\uE000${index}\uE001` : piece.text.replace(/[\uE000\uE001]/g, "\uFFFD"))).join("");
  const html = purify.sanitize(marked.parse(source, { async: false, breaks: true }), documentAllowed);
  return html.replace(/\uE000(\d+)\uE001/g, (found, index: string) => {
    const piece = pieces[Number(index)];
    if (!piece?.token) return "";
    const { className, title } = look(piece);
    return `<button type="button" data-piece="${index}" class="${escaped(className)}" title="${escaped(title)}">${escaped(piece.token)}</button>`;
  });
}

if (import.meta.vitest) {
  const { test, expect } = import.meta.vitest;

  test("a reply keeps its simple formatting and loses everything that could run or load", async () => {
    const { JSDOM } = await import("jsdom");
    const purify = DOMPurify(new JSDOM("").window);
    const html = render(
      'Done. **Agenda** has:\n- a list\n- `code`\n\n<img src=x onerror="alert(1)"><script>alert(2)</script>[link](javascript:alert(3)) <b onclick="x()">bold</b>',
      purify,
    );
    expect(html).toContain("<strong>Agenda</strong>");
    expect(html).toContain("<li>a list</li>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("link");
    expect(html).toContain("bold");
    for (const banned of ["<img", "<script", "alert(2)", "javascript:", "onerror", "onclick", "<a", "<b"]) expect(html).not.toContain(banned);
  });

  test("restored names are underlined, unknown tokens are red, and nothing else changes", async () => {
    const { JSDOM } = await import("jsdom");
    const html = render("Hi **Acme & Co** and ⟦NOPE_1⟧ and `code`", DOMPurify(new JSDOM("").window));
    const done = withMarks(html, [{ text: "Acme & Co", title: "Customer" }, { text: "code", title: "Project \"x\"" }, { text: " ", title: "x" }], "Not known");
    expect(done).toContain('<strong><span class="underline decoration-dotted underline-offset-2" title="Customer">Acme &amp; Co</span></strong>');
    expect(done).toContain('<span class="text-destructive" title="Not known">⟦NOPE_1⟧</span>');
    expect(done).toContain('title="Project &#34;x&#34;"');
    expect(withMarks("<p>plain</p>", [], "x")).toBe("<p>plain</p>");
  });

  test("only a long reply is folded", () => {
    expect(longReply("Short answer.")).toBe(false);
    expect(longReply("x".repeat(601))).toBe(true);
    expect(longReply("line\n".repeat(12))).toBe(true);
  });

  test("a document keeps its headings, lists and tables, and hidden names become buttons", async () => {
    const { JSDOM } = await import("jsdom");
    const purify = DOMPurify(new JSDOM("").window);
    const pieces: Piece[] = [
      { text: "## Slide 1\n\n- Call ", token: null, kind: null },
      { text: "Tom", token: "⟦PERSON_1⟧", kind: "person" },
      { text: " today\n\n| Name | Role |\n| --- | --- |\n| ", token: null, kind: null },
      { text: "Ann", token: "⟦PERSON_2⟧", kind: "person" },
      { text: ' | <img src=x onerror="alert(1)"> \uE0001\uE001 |', token: null, kind: null },
    ];
    const html = piecesHtml(pieces, (piece) => ({ className: `token ${piece.kind}`, title: `${piece.text} "x"` }), purify);
    expect(html).toContain("<h2>Slide 1</h2>");
    expect(html).toContain('<li>Call <button type="button" data-piece="1" class="token person" title="Tom &#34;x&#34;">⟦PERSON_1⟧</button> today</li>');
    expect(html).toContain("<th>Name</th>");
    expect(html).toContain('<td><button type="button" data-piece="3"');
    expect(html.match(/<button/g)).toHaveLength(2);
    for (const banned of ["<img", "onerror"]) expect(html).not.toContain(banned);
  });

  test("only extracted documents and markdown files are shown as documents", () => {
    expect(structured("chat/1-plan.docx.txt")).toBe(true);
    expect(structured("chat/2-report.PDF")).toBe(true);
    expect(structured("chat/3-notes.txt")).toBe(false);
    expect(structured(undefined)).toBe(false);
  });
}
