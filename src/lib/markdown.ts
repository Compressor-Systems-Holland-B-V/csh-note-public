import DOMPurify from "dompurify";
import { marked } from "marked";

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
}
