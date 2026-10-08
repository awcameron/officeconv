"""Writes the small inputs scripts/smoke.sh converts: one file for each kind officeconv reads.

They're built here rather than committed, so no binary test files live in the repo, and they
use only the standard library, so any runner with Python can make them.

Usage: python3 scripts/smoke-inputs.py DIR
"""

import sys
import zipfile
from pathlib import Path

REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PACKAGE_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
DRAWING = "http://schemas.openxmlformats.org/drawingml/2006/main"
PRESENTATION = "http://schemas.openxmlformats.org/presentationml/2006/main"
SHEET = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
WORD = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"


def rels(*entries):
    """A .rels part from (id, kind, target) entries."""
    items = "".join(
        f'<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"/>'
        for id, kind, target in entries
    )
    return f'<Relationships xmlns="{PACKAGE_REL}">{items}</Relationships>'


def content_types(*overrides):
    """[Content_Types].xml, with an override for each (part, content type)."""
    items = "".join(
        f'<Override PartName="/{part}" ContentType="{kind}"/>' for part, kind in overrides
    )
    return (
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" '
        'ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        f"{items}</Types>"
    )


def package(path, parts):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zip:
        for name, contents in parts.items():
            zip.writestr(name, contents)


def xlsx(path):
    """One sheet: a header row, then a text and a number cell."""
    cell = lambda ref, text: f'<c r="{ref}" t="inlineStr"><is><t>{text}</t></is></c>'
    rows = (
        f'<row r="1">{cell("A1", "Region")}{cell("B1", "Units")}</row>'
        f'<row r="2">{cell("A2", "North")}<c r="B2"><v>12</v></c></row>'
    )
    package(
        path,
        {
            "[Content_Types].xml": content_types(
                (
                    "xl/workbook.xml",
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
                ),
            ),
            "_rels/.rels": rels(("rId1", "officeDocument", "xl/workbook.xml")),
            "xl/workbook.xml": (
                f'<workbook xmlns="{SHEET}" xmlns:r="{REL}">'
                '<sheets><sheet name="Sales" sheetId="1" r:id="rId1"/></sheets></workbook>'
            ),
            "xl/_rels/workbook.xml.rels": rels(("rId1", "worksheet", "worksheets/sheet1.xml")),
            "xl/worksheets/sheet1.xml": f'<worksheet xmlns="{SHEET}"><sheetData>{rows}</sheetData></worksheet>',
        },
    )


def docx(path):
    """A heading and a paragraph with bold text."""
    body = (
        '<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Notes</w:t></w:r></w:p>'
        '<w:p><w:r><w:t xml:space="preserve">Smoke </w:t></w:r>'
        "<w:r><w:rPr><w:b/></w:rPr><w:t>test</w:t></w:r></w:p>"
    )
    package(
        path,
        {
            "[Content_Types].xml": content_types(
                (
                    "word/document.xml",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
                )
            ),
            "_rels/.rels": rels(("rId1", "officeDocument", "word/document.xml")),
            "word/document.xml": f'<w:document xmlns:w="{WORD}"><w:body>{body}</w:body></w:document>',
        },
    )


def pptx(path):
    """One slide with a title and a bulleted body."""
    ns = f'xmlns:a="{DRAWING}" xmlns:p="{PRESENTATION}" xmlns:r="{REL}"'
    shape = lambda kind, text: (
        f'<p:sp><p:nvSpPr><p:cNvPr id="2" name="{kind}"/><p:cNvSpPr/>'
        f'<p:nvPr><p:ph type="{kind}"/></p:nvPr></p:nvSpPr>'
        f"<p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"
    )
    slide = (
        f"<p:sld {ns}><p:cSld><p:spTree>"
        f'{shape("title", "Agenda")}{shape("body", "Smoke test")}'
        "</p:spTree></p:cSld></p:sld>"
    )
    package(
        path,
        {
            "[Content_Types].xml": content_types(
                (
                    "ppt/presentation.xml",
                    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
                ),
                (
                    "ppt/slides/slide1.xml",
                    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
                ),
            ),
            "_rels/.rels": rels(("rId1", "officeDocument", "ppt/presentation.xml")),
            "ppt/presentation.xml": (
                f'<p:presentation {ns}><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>'
                "</p:presentation>"
            ),
            "ppt/_rels/presentation.xml.rels": rels(("rId2", "slide", "slides/slide1.xml")),
            "ppt/slides/slide1.xml": slide,
        },
    )


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: smoke-inputs.py DIR")
    out = Path(sys.argv[1])
    (out / "sales.csv").write_text("Region,Units\nNorth,12\n", encoding="utf-8")
    xlsx(out / "sales.xlsx")
    docx(out / "notes.docx")
    pptx(out / "talk.pptx")


if __name__ == "__main__":
    main()
