//! Splits a SQL file into single statements for the `Db` port (D1 runs one
//! statement per prepared statement).
//!
//! The scanner skips `--` and `/* */` comments and quoted strings or
//! identifiers (`'..'`, `".."`, `` `..` ``, `[..]`), and tokenises the rest
//! into words so keywords are matched whole (`backend` is not `END`). A `;`
//! ends a statement only at block depth 0, where `CASE` opens a block
//! anywhere and `BEGIN` opens one inside `CREATE [TEMP|TEMPORARY] TRIGGER`;
//! `END` closes the innermost block.

/// The statements of `sql`, trimmed, without comment-only fragments.
pub(crate) fn split_sql(sql: &str) -> Vec<String> {
    let mut s = Splitter::default();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek() == Some(&'-') => {
                s.end_word();
                for c in chars.by_ref() {
                    if c == '\n' {
                        s.cur.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                s.end_word();
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                s.cur.push(' ');
            }
            '\'' | '"' | '`' | '[' => {
                s.end_word();
                let close = if c == '[' { ']' } else { c };
                s.cur.push(c);
                for q in chars.by_ref() {
                    s.cur.push(q);
                    if q == close {
                        break;
                    }
                }
            }
            ';' => {
                s.end_word();
                if s.depth == 0 {
                    s.finish();
                } else {
                    s.cur.push(';');
                }
            }
            c if c.is_alphanumeric() || c == '_' || c == '$' => {
                s.word.push(c);
                s.cur.push(c);
            }
            c => {
                s.end_word();
                s.cur.push(c);
            }
        }
    }
    s.end_word();
    s.finish();
    s.out
}

#[derive(Default)]
struct Splitter {
    out: Vec<String>,
    cur: String,
    word: String,
    /// The first words of the current statement (enough to spot a trigger).
    head: Vec<String>,
    depth: u32,
}

impl Splitter {
    fn is_trigger(&self) -> bool {
        let h: Vec<&str> = self.head.iter().map(String::as_str).collect();
        matches!(
            h.as_slice(),
            ["CREATE", "TRIGGER", ..] | ["CREATE", "TEMP" | "TEMPORARY", "TRIGGER", ..]
        )
    }

    fn end_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let w = std::mem::take(&mut self.word).to_ascii_uppercase();
        match w.as_str() {
            "CASE" => self.depth += 1,
            "BEGIN" if self.is_trigger() => self.depth += 1,
            "END" => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        if self.head.len() < 3 {
            self.head.push(w);
        }
    }

    fn finish(&mut self) {
        let s = self.cur.trim();
        if !s.is_empty() {
            self.out.push(s.to_string());
        }
        self.cur.clear();
        self.head.clear();
        self.depth = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::split_sql;

    #[test]
    fn splits_statements_and_skips_comments() {
        let sql = "-- header; not a statement\nCREATE TABLE a (x TEXT DEFAULT ';');\n\
                   /* block; */ CREATE INDEX i ON a (x);\n-- trailing\n";
        assert_eq!(
            split_sql(sql),
            vec![
                "CREATE TABLE a (x TEXT DEFAULT ';')".to_string(),
                "CREATE INDEX i ON a (x)".to_string(),
            ]
        );
    }

    #[test]
    fn keeps_trigger_bodies_whole() {
        let sql =
            "CREATE TRIGGER t BEFORE DELETE ON a BEGIN SELECT RAISE(ABORT, 'no'); END;\nSELECT 1;";
        let parts = split_sql(sql);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[0].ends_with("END"), "{parts:?}");
    }

    #[test]
    fn case_inside_a_trigger_body_does_not_end_it() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON a BEGIN \
                   UPDATE a SET n = CASE WHEN n > 0 THEN 1 ELSE 0 END; \
                   UPDATE a SET m = 2; END; SELECT 2;";
        let parts = split_sql(sql);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[0].contains("SET m = 2; END"), "{parts:?}");
    }

    #[test]
    fn identifiers_ending_in_end_are_not_keywords() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON a BEGIN \
                   UPDATE a SET backend = 1; UPDATE a SET legend = \"x END\"; END;\n\
                   CREATE TABLE b (frontend TEXT);";
        let parts = split_sql(sql);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[1].starts_with("CREATE TABLE b"), "{parts:?}");
    }

    #[test]
    fn temp_triggers_are_recognised() {
        for kw in ["TEMP", "temporary"] {
            let sql = format!(
                "CREATE {kw} TRIGGER t BEFORE DELETE ON a BEGIN SELECT 1; SELECT 2; END; SELECT 3;"
            );
            let parts = split_sql(&sql);
            assert_eq!(parts.len(), 2, "{kw}: {parts:?}");
        }
    }

    #[test]
    fn case_in_a_plain_statement_and_begin_outside_triggers() {
        let parts = split_sql("SELECT CASE WHEN 1 THEN 'a' END; BEGIN; SELECT 1;");
        assert_eq!(
            parts,
            ["SELECT CASE WHEN 1 THEN 'a' END", "BEGIN", "SELECT 1"]
        );
    }
}
