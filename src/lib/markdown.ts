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

  test("only a long reply is folded", () => {
    expect(longReply("Short answer.")).toBe(false);
    expect(longReply("x".repeat(601))).toBe(true);
    expect(longReply("line\n".repeat(12))).toBe(true);
  });
}
