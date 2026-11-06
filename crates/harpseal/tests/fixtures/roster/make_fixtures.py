# Builds seal_bay_50.csv and seal_bay_50.xlsx (50 fictional Seal Bay
# University students) in the directory given as the first argument.
# Run once with `python3 make_fixtures.py .`; the outputs are committed.
import sys, zipfile
from xml.sax.saxutils import escape
out = sys.argv[1]
first = ["นกน้อย","ปลาทอง","หอยมุก","กุ้งนาง","ปูม้า","ม้าน้ำ","ดาวทะเล","สาหร่าย","ปะการัง","เกลือทอง"]
last = ["ทะเลใส","อ่าวคราม","หาดทราย","คลื่นลม","ฟองคลื่น"]
H = ["รหัสนักศึกษา","ชื่อ-สกุล","อีเมล","ภาษา","กลุ่ม","หลักสูตร","ชั้นปี","เกรดเฉลี่ย"]
rows = []
for i in range(1, 51):
    code = 68010000 + i
    name = f"{first[(i-1)%10]} {last[(i-1)//10]}"
    email = "" if i in (10, 20) else f"s{code}@student.sealbay.example"
    locale = "en" if i % 7 == 0 else ("" if i % 11 == 0 else "th")
    prog = "DDS-Intl" if i % 4 == 0 else "DDS"
    year = (i - 1) % 6 + 1
    tags = f"ปี{year};{prog}" + (";ชมรมดนตรี" if i % 5 == 0 else "")
    gpa = "" if i % 9 == 0 else f"{2.5 + (i % 15) / 10:.2f}"
    rows.append([str(code), name, email, locale, tags, prog, str(year), gpa])

def q(c):
    return '"' + c.replace('"', '""') + '"' if any(x in c for x in ',;"\r\n') else c
with open(f"{out}/seal_bay_50.csv", "w", encoding="utf-8-sig", newline="") as f:
    for r in [H] + rows:
        f.write(",".join(q(c) for c in r) + "\r\n")

def col(n):
    return "ABCDEFGHIJ"[n]
def cell(ref, v, numeric):
    if v == "":
        return ""
    if numeric:
        return f'<c r="{ref}"><v>{v}</v></c>'
    return f'<c r="{ref}" t="inlineStr"><is><t>{escape(v)}</t></is></c>'
xml_rows = []
for ri, r in enumerate([H] + rows, start=1):
    cells = "".join(cell(f"{col(ci)}{ri}", v, ri > 1 and ci in (0, 6, 7)) for ci, v in enumerate(r))
    xml_rows.append(f'<row r="{ri}">{cells}</row>')
sheet = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
    + "".join(xml_rows) + '</sheetData></worksheet>')
parts = {
 "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
 "_rels/.rels": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
 "xl/workbook.xml": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="รายชื่อ" sheetId="1" r:id="rId1"/></sheets></workbook>',
 "xl/_rels/workbook.xml.rels": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
 "xl/worksheets/sheet1.xml": sheet,
}
def write_xlsx(path, sheet_xml):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in {**parts, "xl/worksheets/sheet1.xml": sheet_xml}.items():
            zi = zipfile.ZipInfo(name, date_time=(2026, 11, 1, 0, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(zi, data)

def sheet_of(rows_xml):
    return ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>'
        + rows_xml + '</sheetData></worksheet>')

write_xlsx(f"{out}/seal_bay_50.xlsx", sheet)
# Two cells far apart: a dense range would be ~17e9 cells.
write_xlsx(f"{out}/sparse.xlsx", sheet_of(
    '<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>'
    '<row r="1048576"><c r="XFD1048576"><v>1</v></c></row>'))
# A first sheet with no cells at all.
write_xlsx(f"{out}/empty_sheet.xlsx", sheet_of(""))
# The headers, then one row whose year is a formula (cached 2) and whose
# GPA is an error cell.
header = "".join(cell(f"{col(ci)}1", v, False) for ci, v in enumerate(H))
data = (cell("A2", "68010001", True) + cell("B2", "นกน้อย ทะเลใส", False)
    + cell("F2", "DDS", False) + '<c r="G2"><f>1+1</f><v>2</v></c>'
    + '<c r="H2" t="e"><v>#DIV/0!</v></c>')
write_xlsx(f"{out}/formula_error.xlsx", sheet_of(
    f'<row r="1">{header}</row><row r="2">{data}</row>'))
