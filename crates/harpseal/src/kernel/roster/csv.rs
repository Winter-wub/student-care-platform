//! A small RFC 4180 reader: quoted fields, `""` inside quotes, CRLF or LF
//! line ends, line breaks inside quotes, and `,` or `;` as the delimiter
//! (whichever appears more often outside quotes on the header line; `,`
//! on a tie). No crate: the workspace list has no CSV parser.

/// Why the text is not CSV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CsvError {
    /// A quoted field never closes; the record starts on this row.
    Unterminated(usize),
    /// Something other than a delimiter or line end follows a closing
    /// quote on this row.
    AfterQuote(usize),
}

/// The delimiter the header line uses.
pub(crate) fn delimiter(text: &str) -> char {
    let (mut commas, mut semis, mut quoted) = (0usize, 0usize, false);
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => commas += 1,
            ';' if !quoted => semis += 1,
            '\n' | '\r' if !quoted => break,
            _ => {}
        }
    }
    if semis > commas { ';' } else { ',' }
}

/// Splits `text` into records of raw (untrimmed) fields. A last line
/// without a line end is a record; a trailing line end adds none.
pub(crate) fn records(text: &str, delim: char) -> Result<Vec<Vec<String>>, CsvError> {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    // `quoted`: inside quotes; `closed`: just after a closing quote.
    let (mut quoted, mut closed, mut started) = (false, false, false);
    let mut row = 1usize;
    let mut record_row = 1usize;
    while let Some(c) = chars.next() {
        started = true;
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    quoted = false;
                    closed = true;
                }
                '\n' => {
                    row += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            c if c == delim => {
                record.push(std::mem::take(&mut field));
                closed = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut record));
                closed = false;
                started = false;
                row += 1;
                record_row = row;
            }
            _ if closed => return Err(CsvError::AfterQuote(row)),
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(CsvError::Unterminated(record_row));
    }
    if started {
        record.push(field);
        out.push(record);
    }
    Ok(out)
}

/// One CSV line (CRLF-terminated, `,`-separated) of `fields`, quoting a
/// field that holds `,`, `;`, `"`, a line break or edge spaces.
pub(crate) fn line(fields: &[&str]) -> String {
    let quoted: Vec<String> = fields
        .iter()
        .map(|f| {
            let needs = f.contains([',', ';', '"', '\n', '\r']) || f.trim() != *f;
            if needs {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                (*f).to_string()
            }
        })
        .collect();
    format!("{}\r\n", quoted.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<Vec<String>> {
        records(text, delimiter(text)).unwrap()
    }

    #[test]
    fn plain_and_quoted_fields() {
        assert_eq!(
            rows("a,b,c\r\n1,\"x, y\",3\r\n"),
            vec![vec!["a", "b", "c"], vec!["1", "x, y", "3"]]
        );
        assert_eq!(
            rows("a,b\n\"say \"\"hi\"\"\",\"\"\n"),
            vec![vec!["a", "b"], vec!["say \"hi\"", ""]]
        );
    }

    #[test]
    fn line_breaks_inside_quotes_and_a_last_line_without_end() {
        assert_eq!(
            rows("a,b\n\"two\r\nlines\",x"),
            vec![vec!["a", "b"], vec!["two\r\nlines", "x"]]
        );
    }

    #[test]
    fn semicolons_are_detected_from_the_header() {
        let text = "รหัส;ชื่อ;\"a,b\"\n1;นก;x,y\n";
        assert_eq!(delimiter(text), ';');
        assert_eq!(rows(text)[1], vec!["1", "นก", "x,y"]);
        assert_eq!(delimiter("a,b;c\n"), ',');
        assert_eq!(delimiter("\"a;b;c\",d\n"), ',');
    }

    #[test]
    fn empty_lines_are_empty_records() {
        assert_eq!(rows("a\n\nb\n"), vec![vec!["a"], vec![""], vec!["b"]]);
        assert!(rows("").is_empty());
    }

    #[test]
    fn lines_quote_what_needs_it() {
        assert_eq!(
            line(&["a", "b c", "x,y", "say \"hi\"", " pad"]),
            "a,b c,\"x,y\",\"say \"\"hi\"\"\",\" pad\"\r\n"
        );
        let back = rows(&line(&["1", "x;y", "z"]));
        assert_eq!(back, vec![vec!["1", "x;y", "z"]]);
    }

    #[test]
    fn broken_quotes_are_refused_with_their_row() {
        assert_eq!(
            records("a\n\"open,x\ny\n", ','),
            Err(CsvError::Unterminated(2))
        );
        assert_eq!(
            records("a,b\n1,2\n\"q\"x,3\n", ','),
            Err(CsvError::AfterQuote(3))
        );
    }
}
