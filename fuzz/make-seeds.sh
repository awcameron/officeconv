#!/bin/sh
# Writes a small document, deck, workbook and CSV file into corpus/<target>/, as starting points
# for the fuzzer. Between them they use headings, lists, tables, links, images, notes, several
# sheets, a sheet picture and CSV quoting, so the fuzzer starts from inputs that reach each part
# of the readers.
#
# Run from fuzz/: ./make-seeds.sh
set -eu

cd "$(dirname "$0")"
here=$(pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

REL=http://schemas.openxmlformats.org/officeDocument/2006/relationships
W=http://schemas.openxmlformats.org/wordprocessingml/2006/main
A=http://schemas.openxmlformats.org/drawingml/2006/main
P=http://schemas.openxmlformats.org/presentationml/2006/main
S=http://schemas.openxmlformats.org/spreadsheetml/2006/main
XDR=http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing

# put <package dir> <part> <contents>: writes one part.
put() {
    mkdir -p "$(dirname "$1/$2")"
    printf '%s' "$3" > "$1/$2"
}

# relationship <id> <type> <target>
relationship() {
    printf '<Relationship Id="%s" Type="%s/%s" Target="%s"/>' "$1" "$REL" "$2" "$3"
}

# rels <relationship>...: a relationships part.
rels() {
    printf '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">%s</Relationships>' "$*"
}

# png <file>: a valid 2x2 red PNG.
png() {
    mkdir -p "$(dirname "$1")"
    printf '\211PNG\r\n\032\n\000\000\000\rIHDR\000\000\000\002\000\000\000\002\010\002\000\000\000\375\324\232s\000\000\000\020IDATx\234c\374\317\000\002L`\222\001\000\r\035\001\003\202\311q\377\000\000\000\000IEND\256B`\202' > "$1"
}

# pack <package dir> <output file>
pack() {
    mkdir -p "$(dirname "$2")"
    rm -f "$2"
    (cd "$1" && zip -qrX "$2" .)
}

# A Word document.
d=$work/docx
put "$d" word/document.xml "<w:document xmlns:w=\"$W\" xmlns:r=\"$REL\" xmlns:a=\"$A\"><w:body>\
<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Notes</w:t></w:r></w:p>\
<w:p><w:r><w:t xml:space=\"preserve\">Ship the </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>beta</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t> soon</w:t></w:r></w:p>\
<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>\
<w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Nested</w:t></w:r></w:p>\
<w:p><w:hyperlink r:id=\"rId1\"><w:r><w:t>Rust</w:t></w:r></w:hyperlink></w:p>\
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Crate</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Use</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>zip</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Café Ωμέγα</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:p><w:r><w:drawing><wp:docPr xmlns:wp=\"wp\" id=\"1\" name=\"p\" descr=\"Logo\"/><a:blip r:embed=\"rId2\"/></w:drawing></w:r></w:p>\
</w:body></w:document>"
put "$d" word/_rels/document.xml.rels "$(rels \
    "$(relationship rId1 hyperlink https://www.rust-lang.org)" \
    "$(relationship rId2 image media/image1.png)")"
put "$d" word/numbering.xml "<w:numbering xmlns:w=\"$W\">\
<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl><w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
put "$d" word/styles.xml "<w:styles xmlns:w=\"$W\"><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/></w:style></w:styles>"
png "$d/word/media/image1.png"
pack "$d" "$here/corpus/docx/seed.docx"

# A PowerPoint deck: a slide with a title, a bulleted body, a link, a picture and notes, then a
# second slide.
p=$work/pptx
NS="xmlns:a=\"$A\" xmlns:p=\"$P\" xmlns:r=\"$REL\""
shape() {
    printf '<p:sp><p:nvSpPr><p:cNvPr id="2" name="%s"/><p:cNvSpPr/><p:nvPr><p:ph type="%s"/></p:nvPr></p:nvSpPr><p:txBody>%s</p:txBody></p:sp>' "$1" "$1" "$2"
}
put "$p" ppt/presentation.xml "<p:presentation $NS><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/><p:sldId id=\"257\" r:id=\"rId2\"/></p:sldIdLst></p:presentation>"
put "$p" ppt/_rels/presentation.xml.rels "$(rels \
    "$(relationship rId1 slide slides/slide1.xml)" \
    "$(relationship rId2 slide slides/slide2.xml)")"
put "$p" ppt/slides/slide1.xml "<p:sld $NS><p:cSld><p:spTree>\
$(shape title '<a:p><a:r><a:t>Agenda</a:t></a:r></a:p>')\
$(shape body '<a:p><a:r><a:t>Read the book</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>Rust</a:t></a:r></a:p>')\
<p:pic><p:nvPicPr><p:cNvPr id=\"3\" name=\"Picture\" descr=\"Logo\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId3\"/></p:blipFill></p:pic>\
</p:spTree></p:cSld></p:sld>"
put "$p" ppt/slides/_rels/slide1.xml.rels "$(rels \
    "$(relationship rId1 notesSlide ../notesSlides/notesSlide1.xml)" \
    "$(relationship rId2 hyperlink https://www.rust-lang.org)" \
    "$(relationship rId3 image ../media/image1.png)")"
put "$p" ppt/slides/slide2.xml "<p:sld $NS><p:cSld><p:spTree>$(shape title '<a:p><a:r><a:t>Thanks</a:t></a:r></a:p>')</p:spTree></p:cSld></p:sld>"
put "$p" ppt/notesSlides/notesSlide1.xml "<p:notes $NS><p:cSld><p:spTree>$(shape body '<a:p><a:r><a:t>Keep it short.</a:t></a:r></a:p>')</p:spTree></p:cSld></p:notes>"
png "$p/ppt/media/image1.png"
pack "$p" "$here/corpus/pptx/seed.pptx"

# An Excel workbook: two sheets, shared and inline strings, numbers, a boolean, a formula, and
# a picture on the first sheet.
x=$work/xlsx
put "$x" "[Content_Types].xml" '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>'
put "$x" _rels/.rels "$(rels "$(relationship rId1 officeDocument xl/workbook.xml)")"
put "$x" xl/workbook.xml "<workbook xmlns=\"$S\" xmlns:r=\"$REL\"><sheets><sheet name=\"Sales\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Notes\" sheetId=\"2\" r:id=\"rId2\"/></sheets></workbook>"
put "$x" xl/_rels/workbook.xml.rels "$(rels \
    "$(relationship rId1 worksheet worksheets/sheet1.xml)" \
    "$(relationship rId2 worksheet worksheets/sheet2.xml)" \
    "$(relationship rId3 sharedStrings sharedStrings.xml)")"
put "$x" xl/sharedStrings.xml "<sst xmlns=\"$S\"><si><t>Region</t></si><si><t>Total</t></si><si><t>North | \"east\"</t></si></sst>"
put "$x" xl/worksheets/sheet1.xml "<worksheet xmlns=\"$S\" xmlns:r=\"$REL\"><sheetData>\
<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c><c r=\"C1\" t=\"inlineStr\"><is><t>Done</t></is></c></row>\
<row r=\"2\"><c r=\"A2\" t=\"s\"><v>2</v></c><c r=\"B2\"><v>12.5</v></c><c r=\"C2\" t=\"b\"><v>1</v></c></row>\
<row r=\"4\"><c r=\"A4\" t=\"str\"><f>A2</f><v>North</v></c><c r=\"B4\"><f>B2*2</f><v>25</v></c><c r=\"C4\" t=\"e\"><v>#DIV/0!</v></c></row>\
</sheetData><drawing r:id=\"rId1\"/></worksheet>"
put "$x" xl/worksheets/_rels/sheet1.xml.rels "$(rels "$(relationship rId1 drawing ../drawings/drawing1.xml)")"
put "$x" xl/worksheets/sheet2.xml "<worksheet xmlns=\"$S\"><sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Café</t></is></c></row></sheetData></worksheet>"
put "$x" xl/drawings/drawing1.xml "<xdr:wsDr xmlns:xdr=\"$XDR\" xmlns:a=\"$A\" xmlns:r=\"$REL\"><xdr:twoCellAnchor>\
<xdr:from><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>\
<xdr:to><xdr:col>6</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>5</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>\
<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"2\" name=\"Picture 1\" descr=\"Chart\"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"rId1\"/></xdr:blipFill></xdr:pic>\
<xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"
put "$x" xl/drawings/_rels/drawing1.xml.rels "$(rels "$(relationship rId1 image ../media/image1.png)")"
png "$x/xl/media/image1.png"
pack "$x" "$here/corpus/xlsx/seed.xlsx"

# A CSV file with the quoting a reader has to get right, read as TSV too.
mkdir -p corpus/delimited
printf '\357\273\277Name,Note,Size\r\nAda,"Hello, world",5" screen\r\n"Alan ""AT"" Turing","two\nlines"\r\nTab\there,,,extra\n' \
    > corpus/delimited/seed.csv

# The archive target gets all three.
mkdir -p corpus/archive
cp corpus/docx/seed.docx corpus/pptx/seed.pptx corpus/xlsx/seed.xlsx corpus/archive/

echo "wrote seeds to $here/corpus"
