#!/usr/bin/env python3
"""Builds the plain one-page PDFs the demo attaches. Everything in them is made up.
The encrypted .znote file the demo opens comes from src-tauri/examples/demo_znote.rs.

    python3 tools/demo/fixtures.py OUT_DIR
"""
import sys
from pathlib import Path

# Plain text pages, so the PDF text layer reads back cleanly and the token
# view has simple paragraphs to show.
PDFS = {
    "Quarterly report.pdf": [
        ("Larkspur Bakery - Quarterly report Q3", 18),
        ("", 11),
        ("Prepared by Maya Lindqvist, finance lead, for the board of Larkspur Bakery.", 11),
        ("", 11),
        ("Revenue reached EUR 412,800 this quarter, up 9 percent on Q2.", 11),
        ("Online orders grew to 31 percent of sales after the new ordering app pilot.", 11),
        ("Flour costs from Millbrook Grain Co. rose by EUR 18,400 after the July price change.", 11),
        ("The Utrecht store was closed for six days for oven repairs.", 11),
        ("", 11),
        ("Open points", 13),
        ("- Renegotiate the Millbrook Grain Co. contract before 1 December.", 11),
        ("- Hire two bakers for the Haarlem store; contact Jonas Verbeek (jonas@larkspur.example).", 11),
        ("- Decide on the delivery partner: Quickbike or CityCourier.", 11),
    ],
    "Supplier letter.pdf": [
        ("Millbrook Grain Co.", 18),
        ("", 11),
        ("To: Maya Lindqvist, Larkspur Bakery, Keizersgracht 12, Amsterdam", 11),
        ("", 11),
        ("Dear Ms Lindqvist,", 11),
        ("", 11),
        ("From 1 January our price for stone-ground flour goes up from EUR 0.82 to EUR 0.91 per kg.", 11),
        ("Orders above 20 tonnes per quarter keep the current price for six more months.", 11),
        ("Please reply to Pieter de Graaf at pieter.degraaf@millbrook.example or +31 20 555 0142.", 11),
        ("", 11),
        ("Kind regards,", 11),
        ("Pieter de Graaf, account manager", 11),
    ],
}


def pdf(lines):
    text = ["BT", "72 770 Td"]
    for line, size in lines:
        safe = line.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
        text += [f"/F1 {size} Tf", f"({safe}) Tj", f"0 -{round(size * 1.6)} Td"]
    text.append("ET")
    stream = "\n".join(text).encode("latin-1")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] "
        b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    out += b"".join(b"%010d 00000 n \n" % offset for offset in offsets)
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    return bytes(out)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for name, lines in PDFS.items():
        (out / name).write_bytes(pdf(lines))
    print(f"==> fixtures in {out}")


if __name__ == "__main__":
    main()
