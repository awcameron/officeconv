#!/usr/bin/env python3
"""Writes valid .docx, .pptx and .xlsx files whose XML nests the elements the readers care about
at random: mostly the way Office does, sometimes not (runs in runs, text outside runs, tables in
odd places, Fallback anywhere). Table cells are merged at random too, sometimes past the table. compare.sh converts them with two builds of officeconv.

Fuzz corpus inputs rarely get this far: most are broken zips. These are always valid packages,
so every one reaches the XML readers.

Usage: gen-nesting.py OUT_DIR COUNT_PER_KIND SEED
Writes OUT_DIR/docx/*.docx, OUT_DIR/pptx/*.pptx and OUT_DIR/xlsx/*.xlsx. The same seed writes
the same files."""

import os
import random
import sys
import zipfile

out, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
rng = random.Random(seed)

REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PNG = (b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x02\x00\x00\x00\x02\x08\x02\x00\x00\x00"
       b"\xfd\xd4\x9as\x00\x00\x00\x10IDATx\x9cc\xfc\xcf\x00\x02L`\x92\x01\x00\r\x1d\x01\x03\x82"
       b"\xc9q\xff\x00\x00\x00\x00IEND\xaeB`\x82")
WORDS = ["Hello", " world", "Café", "a &amp; b", "&#233;t&#233;", " ", "", "x &lt; y", "12", " 3 "]


def rels(*entries):
    body = "".join(f'<Relationship Id="{i}" Type="{REL}/{t}" Target="{target}"{extra}/>'
                   for i, t, target, extra in entries)
    return f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>'


def tree(vocab, children, parent, depth):
    """Random content for `parent`: usually its usual children, sometimes anything."""
    parts = []
    usual = children.get(parent)
    # Elements that hold text (t, row, col) mostly get text.
    text_odds = 0.85 if usual == [] else 0.15
    for _ in range(rng.randint(1 if depth < 4 else 0, 4 if depth < 6 else 1)):
        if rng.random() < text_odds:
            numbers = parent in ("xdr:row", "xdr:col")
            parts.append(str(rng.randint(0, 30)) if numbers else rng.choice(WORDS))
            continue
        name = rng.choice(usual) if usual and rng.random() < 0.85 else rng.choice(list(vocab))
        attrs = vocab[name]()
        if depth >= 7 or rng.random() < 0.1:
            parts.append(f"<{name}{attrs}/>")
        else:
            parts.append(f"<{name}{attrs}>{tree(vocab, children, name, depth + 1)}</{name}>")
    return "".join(parts)


def a(**kw):
    """Attributes, each present half the time, with a value picked from the given choices."""
    def make():
        return "".join(f' {k.replace("_", ":")}="{rng.choice(v)}"' for k, v in kw.items()
                       if rng.random() < 0.5)
    return make


# ---- Word ----
W = {
    "w:p": a(), "w:pPr": a(), "w:pStyle": a(w_val=["Heading1", "Title", "Heading9", "x"]),
    "w:numPr": a(), "w:numId": a(w_val=["1", "2"]), "w:ilvl": a(w_val=["0", "1", "x"]),
    "w:r": a(), "w:rPr": a(), "w:b": a(w_val=["0", "1", "false"]), "w:i": a(w_val=["0", "1"]),
    "w:t": a(), "w:tab": a(), "w:br": a(w_type=["page", "column", "textWrapping"]), "w:cr": a(),
    "w:hyperlink": a(r_id=["rId1", "rId9"]), "w:tbl": a(), "w:tr": a(), "w:tc": a(),
    "mc:AlternateContent": a(), "mc:Choice": a(), "mc:Fallback": a(), "w:pPrChange": a(),
    "w:rPrChange": a(), "w:drawing": a(), "wp:docPr": a(descr=["Logo", ""], title=["T"]),
    "a:blip": a(r_embed=["rId2", "rId9"]), "v:imagedata": a(r_id=["rId2"], o_title=["Old"]),
    "w:txbxContent": a(), "m:r": a(), "m:t": a(), "w:delText": a(), "x:other": a(),
    "w:tblGrid": a(), "w:gridCol": a(), "w:tcPr": a(), "w:tcPrChange": a(),
    "w:gridSpan": a(w_val=["1", "2", "3", "x", "4000000000"]), "w:vMerge": a(w_val=["restart", "continue"]),
}
W_CHILDREN = {
    None: ["w:p", "w:p", "w:tbl"], "w:body": ["w:p", "w:tbl"],
    "w:p": ["w:pPr", "w:r", "w:r", "w:hyperlink", "mc:AlternateContent"],
    "w:pPr": ["w:pStyle", "w:numPr", "w:pPrChange"], "w:numPr": ["w:numId", "w:ilvl"],
    "w:r": ["w:rPr", "w:t", "w:t", "w:tab", "w:br", "w:drawing", "mc:AlternateContent"],
    "w:rPr": ["w:b", "w:i", "w:rPrChange"], "w:t": [], "w:hyperlink": ["w:r"],
    "w:tbl": ["w:tblGrid", "w:tr", "w:tr"], "w:tblGrid": ["w:gridCol"], "w:tr": ["w:tc"],
    "w:tc": ["w:tcPr", "w:p", "w:tbl"], "w:tcPr": ["w:gridSpan", "w:vMerge", "w:tcPrChange"],
    "mc:AlternateContent": ["mc:Choice", "mc:Fallback"], "mc:Choice": ["w:r", "w:drawing"],
    "mc:Fallback": ["w:r", "w:t"], "w:drawing": ["wp:docPr", "a:blip", "w:txbxContent"],
    "w:txbxContent": ["w:p"],
}


def docx(path):
    body = tree(W, W_CHILDREN, None, 0)
    doc = (f'<w:document xmlns:w="w" xmlns:r="{REL}" xmlns:a="a" xmlns:wp="wp" xmlns:mc="mc" '
           f'xmlns:v="v" xmlns:o="o" xmlns:m="m" xmlns:x="x"><w:body>{body}</w:body></w:document>')
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("word/document.xml", doc)
        z.writestr("word/_rels/document.xml.rels", rels(
            ("rId1", "hyperlink", "https://example.com", ' TargetMode="External"'),
            ("rId2", "image", "media/image1.png", "")))
        z.writestr("word/numbering.xml", '<w:numbering xmlns:w="w"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>')
        z.writestr("word/styles.xml", '<w:styles xmlns:w="w"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>')
        z.writestr("word/media/image1.png", PNG)


# ---- PowerPoint ----
P = {
    "p:sp": a(), "p:nvSpPr": a(), "p:cNvPr": a(descr=["Pic", ""], title=["T"]), "p:nvPr": a(),
    "p:ph": a(type=["title", "ctrTitle", "body", "subTitle", "sldNum"]), "p:txBody": a(),
    "a:p": a(), "a:pPr": a(lvl=["0", "1", "x"]), "a:buNone": a(), "a:buChar": a(),
    "a:buAutoNum": a(), "a:r": a(), "a:rPr": a(b=["0", "1", "true"], i=["1"]),
    "a:hlinkClick": a(r_id=["rId2", "rId9"]), "a:t": a(), "a:br": a(), "a:fld": a(),
    "p:graphicFrame": a(), "a:tbl": a(), "a:tblGrid": a(), "a:gridCol": a(), "a:tr": a(),
    "a:tc": a(gridSpan=["1", "2", "3", "x", "4000000000"], rowSpan=["1", "2", "3", "4000000000"],
              hMerge=["1", "0"], vMerge=["1", "true"]),
    "p:pic": a(),
    "p:nvPicPr": a(), "p:blipFill": a(), "a:blip": a(r_embed=["rId3", "rId9"]),
    "mc:AlternateContent": a(), "mc:Choice": a(), "mc:Fallback": a(), "x:other": a(),
}
P_CHILDREN = {
    None: ["p:sp", "p:sp", "p:pic", "p:graphicFrame", "mc:AlternateContent"],
    "p:sp": ["p:nvSpPr", "p:txBody"], "p:nvSpPr": ["p:cNvPr", "p:nvPr"], "p:nvPr": ["p:ph"],
    "p:txBody": ["a:p"], "a:p": ["a:pPr", "a:r", "a:r", "a:br", "a:fld"],
    "a:pPr": ["a:buNone", "a:buChar", "a:buAutoNum"], "a:r": ["a:rPr", "a:t"],
    "a:fld": ["a:rPr", "a:t"], "a:rPr": ["a:hlinkClick"], "a:t": [],
    "p:graphicFrame": ["a:tbl"], "a:tbl": ["a:tblGrid", "a:tr", "a:tr"], "a:tblGrid": ["a:gridCol"],
    "a:tr": ["a:tc"], "a:tc": ["p:txBody"],
    "p:pic": ["p:nvPicPr", "p:blipFill"], "p:nvPicPr": ["p:cNvPr"], "p:cNvPr": ["a:hlinkClick"],
    "p:blipFill": ["a:blip"], "mc:AlternateContent": ["mc:Choice", "mc:Fallback"],
    "mc:Choice": ["p:sp", "p:pic"], "mc:Fallback": ["p:sp", "p:pic"],
}
P_NS = f'xmlns:a="a" xmlns:p="p" xmlns:r="{REL}" xmlns:mc="mc" xmlns:x="x"'


def pptx(path):
    slides = rng.randint(1, 3)
    with zipfile.ZipFile(path, "w") as z:
        ids = "".join(f'<p:sldId id="{256 + i}" r:id="rId{i + 1}"/>' for i in range(slides))
        z.writestr("ppt/presentation.xml", f"<p:presentation {P_NS}><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>")
        z.writestr("ppt/_rels/presentation.xml.rels", rels(
            *[(f"rId{i + 1}", "slide", f"slides/slide{i + 1}.xml", "") for i in range(slides)]))
        for i in range(1, slides + 1):
            show = rng.choice(["", ' show="0"', ' show="1"'])
            z.writestr(f"ppt/slides/slide{i}.xml", f"<p:sld {P_NS}{show}><p:cSld><p:spTree>{tree(P, P_CHILDREN, None, 0)}</p:spTree></p:cSld></p:sld>")
            z.writestr(f"ppt/slides/_rels/slide{i}.xml.rels", rels(
                ("rId1", "notesSlide", f"../notesSlides/notesSlide{i}.xml", ""),
                ("rId2", "hyperlink", "https://example.com", ' TargetMode="External"'),
                ("rId3", "image", "../media/image1.png", "")))
            z.writestr(f"ppt/notesSlides/notesSlide{i}.xml", f"<p:notes {P_NS}><p:cSld><p:spTree>{tree(P, P_CHILDREN, None, 0)}</p:spTree></p:cSld></p:notes>")
        z.writestr("ppt/media/image1.png", PNG)


# ---- Excel drawing ----
X = {
    "xdr:twoCellAnchor": a(), "xdr:oneCellAnchor": a(), "xdr:absoluteAnchor": a(),
    "xdr:from": a(), "xdr:to": a(), "xdr:row": a(), "xdr:col": a(), "xdr:rowOff": a(),
    "xdr:pic": a(), "xdr:nvPicPr": a(), "xdr:cNvPr": a(descr=["Chart", ""], title=["T"]),
    "xdr:blipFill": a(), "a:blip": a(r_embed=["rId1", "rId9"]), "mc:AlternateContent": a(),
    "mc:Fallback": a(), "xdr:clientData": a(), "x:other": a(),
}
X_CHILDREN = {
    None: ["xdr:twoCellAnchor", "xdr:oneCellAnchor", "xdr:absoluteAnchor"],
    "xdr:twoCellAnchor": ["xdr:from", "xdr:to", "xdr:pic", "xdr:clientData"],
    "xdr:oneCellAnchor": ["xdr:from", "xdr:pic"], "xdr:absoluteAnchor": ["xdr:pic"],
    "xdr:from": ["xdr:row", "xdr:col", "xdr:rowOff"], "xdr:to": ["xdr:row", "xdr:col"],
    "xdr:row": [], "xdr:col": [], "xdr:pic": ["xdr:nvPicPr", "xdr:blipFill"],
    "xdr:nvPicPr": ["xdr:cNvPr"], "xdr:blipFill": ["a:blip"],
}
S = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"


def xlsx(path):
    drawing = tree(X, X_CHILDREN, None, 0)
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>')
        z.writestr("_rels/.rels", rels(("rId1", "officeDocument", "xl/workbook.xml", "")))
        z.writestr("xl/workbook.xml", f'<workbook xmlns="{S}" xmlns:r="{REL}"><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/></sheets></workbook>')
        z.writestr("xl/_rels/workbook.xml.rels", rels(("rId1", "worksheet", "worksheets/sheet1.xml", "")))
        z.writestr("xl/worksheets/sheet1.xml", f'<worksheet xmlns="{S}" xmlns:r="{REL}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>A</t></is></c></row></sheetData><drawing r:id="rId1"/></worksheet>')
        z.writestr("xl/worksheets/_rels/sheet1.xml.rels", rels(("rId1", "drawing", "../drawings/drawing1.xml", "")))
        z.writestr("xl/drawings/drawing1.xml", f'<xdr:wsDr xmlns:xdr="xdr" xmlns:a="a" xmlns:r="{REL}" xmlns:mc="mc" xmlns:x="x">{drawing}</xdr:wsDr>')
        z.writestr("xl/drawings/_rels/drawing1.xml.rels", rels(("rId1", "image", "../media/image1.png", "")))
        z.writestr("xl/media/image1.png", PNG)


for kind, make in (("docx", docx), ("pptx", pptx), ("xlsx", xlsx)):
    os.makedirs(f"{out}/{kind}", exist_ok=True)
    for n in range(count):
        make(f"{out}/{kind}/{n:05}.{kind}")
