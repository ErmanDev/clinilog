"""Builds docs/Clinilog-User-Guide.pdf from guide.html.

    python docs/user-guide/build.py

Needs Microsoft Edge (comes with Windows), plus the Python packages pypdf and reportlab.
1. Prints guide.html to PDF with Edge (headless, A4, no browser header/footer).
2. Finds the page where each section starts and writes those numbers into the contents.
3. Prints again and stamps "Clinilog User Guide · Page N" at the bottom (not on the cover).
"""
import io
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from reportlab.lib.colors import HexColor
from reportlab.pdfgen import canvas

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "guide.html"
OUTPUT = HERE.parent / "Clinilog-User-Guide.pdf"
EDGE_CANDIDATES = [
    Path(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
    Path(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
]


def edge() -> Path:
    for p in EDGE_CANDIDATES:
        if p.exists():
            return p
    raise SystemExit("Microsoft Edge not found.")


def print_pdf(html: str, out: Path, work: Path) -> None:
    page = HERE / "_render.html"  # next to guide.html so img/ paths resolve
    page.write_text(html, encoding="utf-8")
    try:
        subprocess.run(
            [str(edge()), "--headless=new", "--disable-gpu", "--no-first-run", "--no-pdf-header-footer",
             f"--user-data-dir={work / 'edge-profile'}", f"--print-to-pdf={out}", page.as_uri()],
            check=True, capture_output=True, timeout=180,
        )
    finally:
        page.unlink(missing_ok=True)
    if not out.exists():
        raise SystemExit("Edge didn't produce a PDF.")


def with_markers(html: str) -> str:
    # An invisible, extractable marker at each section heading, e.g. [[s5]].
    html = html.replace("</style>", ".pdfmark{font-size:1px;color:#fff;}\n</style>")
    return re.sub(r'<h2 id="(s\d+)">', r'<h2 id="\1"><span class="pdfmark">[[\1]]</span>', html)


def fill_contents(html: str, pages: dict) -> str:
    return re.sub(r"\{\{p:(s\d+)\}\}", lambda m: str(pages.get(m.group(1), "")), html)


def section_pages(pdf: Path) -> dict:
    found = {}
    for i, p in enumerate(PdfReader(str(pdf)).pages, start=1):
        for sid in re.findall(r"\[\[(s\d+)\]\]", p.extract_text() or ""):
            found.setdefault(sid, i)
    return found


def stamp_footers(src: Path, dst: Path) -> int:
    reader = PdfReader(str(src))
    writer = PdfWriter()
    for i, page in enumerate(reader.pages, start=1):
        if i > 1:  # no footer on the cover
            w, h = float(page.mediabox.width), float(page.mediabox.height)
            buf = io.BytesIO()
            c = canvas.Canvas(buf, pagesize=(w, h))
            c.setFont("Helvetica", 8)
            c.setFillColor(HexColor("#64748b"))
            c.drawString(42, 22, "Clinilog User Guide")
            c.drawRightString(w - 42, 22, f"Page {i}")
            c.setStrokeColor(HexColor("#e2e8f0"))
            c.line(42, 32, w - 42, 32)
            c.save()
            buf.seek(0)
            page.merge_page(PdfReader(buf).pages[0])
        writer.add_page(page)
    writer.add_metadata({"/Title": "Clinilog User Guide", "/Author": "Clinilog", "/Subject": "How to use Clinilog for clinic staff and administrators"})
    with open(dst, "wb") as f:
        writer.write(f)
    return len(reader.pages)


def main() -> None:
    template = with_markers(SOURCE.read_text(encoding="utf-8"))
    work = Path(tempfile.mkdtemp(prefix="clinilog-guide-"))
    try:
        first = work / "pass1.pdf"
        print_pdf(fill_contents(template, {}), first, work)  # placeholders → blank for pass 1
        pages = section_pages(first)
        missing = [f"s{n}" for n in range(1, 20) if f"s{n}" not in pages]
        if missing:
            raise SystemExit(f"Couldn't find sections: {missing}")
        second = work / "pass2.pdf"
        print_pdf(fill_contents(template, pages), second, work)
        if section_pages(second) != pages:
            raise SystemExit("Page numbers moved between passes; run again.")
        total = stamp_footers(second, OUTPUT)
        print(f"Wrote {OUTPUT} ({total} pages)")
        print("Section pages:", ", ".join(f"{k}={v}" for k, v in sorted(pages.items(), key=lambda kv: int(kv[0][1:]))))
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
