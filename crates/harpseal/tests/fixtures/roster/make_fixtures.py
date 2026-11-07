# Builds seal_bay_50.csv and seal_bay_50.xlsx (50 fictional Seal Bay
# University students), the small XLSX edge cases and the #70 workbooks
# (cohorts*.xlsx, fees.xlsx) in the directory given as the first argument.
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
    prog = "MSC-Intl" if i % 4 == 0 else "MSC"
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
    + cell("F2", "MSC", False) + '<c r="G2"><f>1+1</f><v>2</v></c>'
    + '<c r="H2" t="e"><v>#DIV/0!</v></c>')
write_xlsx(f"{out}/formula_error.xlsx", sheet_of(
    f'<row r="1">{header}</row><row r="2">{data}</row>'))

# --- #70: workbooks with several sheets -----------------------------------
def write_book(path, sheets):
    """sheets: [(name, rows_xml[, hidden])]; every sheet a worksheet."""
    n = len(sheets)
    types = "".join(
        f'<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        for i in range(1, n + 1))
    book = "".join(
        f'<sheet name="{escape(sh[0])}" sheetId="{i}"'
        + (' state="hidden"' if len(sh) > 2 and sh[2] else "") + f' r:id="rId{i}"/>'
        for i, sh in enumerate(sheets, start=1))
    rels = "".join(
        f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>'
        for i in range(1, n + 1))
    files = {
        "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>' + types + '</Types>',
        "_rels/.rels": parts["_rels/.rels"],
        "xl/workbook.xml": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>' + book + '</sheets></workbook>',
        "xl/_rels/workbook.xml.rels": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' + rels + '</Relationships>',
    }
    for i, sh in enumerate(sheets, start=1):
        files[f"xl/worksheets/sheet{i}.xml"] = sheet_of(sh[1])
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in files.items():
            zi = zipfile.ZipInfo(name, date_time=(2026, 11, 1, 0, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(zi, data)

def xrow(r, values):
    """One row: str values as inline text, int/float as numbers, ("e", code)
    as an error cell, ("f", formula, cached float) as a formula cell, None
    as no cell."""
    out = []
    for ci, v in enumerate(values):
        ref = f"{col(ci)}{r}"
        if v is None or v == "":
            continue
        if isinstance(v, tuple) and v[0] == "f":
            # A formula and its cached value, as Excel stores it.
            out.append(f'<c r="{ref}"><f>{escape(v[1])}</f><v>{v[2]!r}</v></c>')
        elif isinstance(v, tuple):
            out.append(f'<c r="{ref}" t="e"><v>{escape(v[1])}</v></c>')
        elif isinstance(v, (int, float)):
            out.append(f'<c r="{ref}"><v>{v}</v></c>')
        else:
            out.append(f'<c r="{ref}" t="inlineStr"><is><t xml:space="preserve">{escape(v)}</t></is></c>')
    return f'<row r="{r}">{"".join(out)}</row>' if out else ""

def student(code, name, year, gpa=None, prog="MSC"):
    return [code, name, f"s{code}@student.sealbay.example", "th", f"ปี{year}", prog, year, gpa]

# Three cohorts: "ปี 1" has two title rows and a totals row (a count
# under the integer column ชั้นปี), "ปี 2" a notes
# row and numbers stored as text, then an empty sheet.
y1 = (xrow(1, ["รายชื่อนักศึกษา มหาวิทยาลัยซีลเบย์"]) + xrow(2, ["ปีการศึกษา 2569"])
    + xrow(4, H)
    + xrow(5, student(68010001, "นกน้อย ทะเลใส", 1, 3.25))
    + xrow(6, student(68010002, "ปลาทอง อ่าวคราม", 1))
    + xrow(7, student(68010003, "หอยมุก หาดทราย", 1, 2.5, "MSC-Intl"))
    + xrow(8, ["รวม", None, None, None, None, None, 3]))
y2 = (xrow(1, H)
    + xrow(2, student(67010001, "กุ้งนาง คลื่นลม", " 2 ", "3.50"))
    + xrow(3, student(67010002, "ปูม้า ฟองคลื่น", "2", " 2.75 "))
    + xrow(5, ["หมายเหตุ: ข้อมูล ณ วันที่ 1 พฤศจิกายน 2569"]))
write_book(f"{out}/cohorts.xlsx", [("ปี 1", y1), ("ปี 2", y2), ("ว่าง", "")])

# A summary sheet without the header beside a good cohort sheet.
summary = xrow(1, ["สรุปจำนวนนักศึกษา"]) + xrow(2, ["ปี 1", 3])
write_book(f"{out}/cohorts_no_header.xlsx", [("ปี 1", y1), ("สรุป", summary)])

# The same id in two sheets, and an error cell in the second.
y2_bad = (xrow(1, H)
    + xrow(2, student(68010002, "ปลาทอง อ่าวคราม", 2))
    + xrow(3, student(67010003, "ม้าน้ำ ทะเลใส", 2, ("e", "#REF!"))))
write_book(f"{out}/cohorts_duplicate.xlsx", [("ปี 1", y1), ("ปี 2", y2_bad)])

# A Module's upload (fees, read with harpseal::sheet): title row, amounts
# stored as text with separators, as numbers, "-" and blank, error cells,
# a totals row and a notes row.
FEES = ["รหัสนักศึกษา", "ชื่อ-สกุล", "ยอดค้างชำระ"]
fees = (xrow(1, ["ค่าธรรมเนียมการศึกษา ภาคเรียนที่ 2/2569"])
    + xrow(3, FEES)
    + xrow(4, [68010001, "นกน้อย ทะเลใส", "12,500"])
    + xrow(5, [68010002, "ปลาทอง อ่าวคราม", " 12500 "])
    + xrow(6, [68010003, "หอยมุก หาดทราย", "12,500.00"])
    + xrow(7, [68010004, "กุ้งนาง คลื่นลม", 12500])
    + xrow(8, [68010005, "ปูม้า ฟองคลื่น", "-"])
    + xrow(9, [68010006, "ม้าน้ำ ทะเลใส", None])
    + xrow(10, [68010007, "ดาวทะเล อ่าวคราม", ("e", "#REF!")])
    + xrow(11, [68010008, "สาหร่าย หาดทราย", ("e", "#N/A")])
    + xrow(12, ["รวม", None, 50000])
    + xrow(13, ["หมายเหตุ: ยอด ณ วันที่ 1 พฤศจิกายน 2569"]))
write_book(f"{out}/fees.xlsx", [("ภาคเรียน 2", fees)])

# Below the data of "ปี 1": a student without an id and a row whose id is
# an error cell (both data, reported), then the totals row (skipped).
trailing = (xrow(4, H)
    + xrow(5, student(68010001, "นกน้อย ทะเลใส", 1, 3.25))
    + xrow(6, [None, "ม้าน้ำ ทะเลใส", "s@student.sealbay.example", "th", "ปี1", "MSC", 1])
    + xrow(7, [("e", "#REF!"), None, None, None, None, None, 2])
    + xrow(8, ["รวม", None, None, None, None, None, 3]))
write_book(f"{out}/cohorts_trailing.xlsx", [("ปี 1", trailing)])

# A hidden lookup sheet beside a cohort sheet.
lookup = xrow(1, ["หลักสูตร"]) + xrow(2, ["MSC"]) + xrow(3, ["MSC-Intl"])
write_book(f"{out}/cohorts_hidden.xlsx", [("ปี 2", y2), ("lists", lookup, True)])

# Calculated cells (#70 review): an =AVERAGE GPA and calculated amounts
# whose cached values carry float noise beyond 15 significant digits.
calc = (xrow(1, H)
    + xrow(2, student(68010001, "นกน้อย ทะเลใส", 1, ("f", "AVERAGE(3,3,4)", 3.3333333333333335)))
    + xrow(3, student(68010002, "ปลาทอง อ่าวคราม", 1, ("f", "AVERAGE(2,3,3)", 2.6666666666666665)))
    + xrow(4, [68010003, "หอยมุก หาดทราย", "s68010003@student.sealbay.example", "th",
               "ปี1", "MSC", ("f", "0.1*10", 1.0000000000000002), 3.0]))
write_book(f"{out}/calculated.xlsx", [("ปี 1", calc)])
calc_fees = (xrow(1, FEES)
    + xrow(2, [68010001, "นกน้อย ทะเลใส", ("f", "12500.1-0.1", 12500.000000000002)])
    + xrow(3, [68010002, "ปลาทอง อ่าวคราม", ("f", "0.1+0.2", 0.30000000000000004)])
    + xrow(4, [68010003, "หอยมุก หาดทราย", ("f", "10/3", 3.3333333333333335)])
    + xrow(5, ["รวม", None, ("f", "SUM(C2:C4)", 12503.633333333333)]))
write_book(f"{out}/calculated_fees.xlsx", [("ภาคเรียน 2", calc_fees)])

# A hidden sheet that holds the roster header beside a visible cohort, and
# a workbook whose only content is in a hidden sheet.
write_book(f"{out}/cohorts_hidden_roster.xlsx", [("ปี 1", y1), ("ปี 2", y2, True)])
write_book(f"{out}/only_hidden.xlsx", [("ว่าง", ""), ("ปี 2", y2, True)])
