//! Minimal RFC 4180 CSV reading and writing, shared by alias import/export, the table
//! export and the event log export.

use std::borrow::Cow;

use thiserror::Error;

/// Records end with CRLF as RFC 4180 specifies; every spreadsheet reads it.
pub const RECORD_END: &str = "\r\n";

/// Quotes a field when it contains the delimiter, a quote, CR or LF (RFC 4180 section 2).
pub fn escape_field(field: &str, delimiter: char) -> Cow<'_, str> {
    if field.contains([delimiter, '"', '\r', '\n']) {
        Cow::Owned(format!("\"{}\"", field.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(field)
    }
}

/// One record, escaped and terminated.
pub fn write_record<S: AsRef<str>>(fields: &[S], delimiter: char) -> String {
    let mut line = fields.iter().map(|f| escape_field(f.as_ref(), delimiter)).collect::<Vec<_>>().join(&delimiter.to_string());
    line.push_str(RECORD_END);
    line
}

/// Defuses spreadsheet formula injection: a cell that would be read as a formula (leading
/// `=`, `+`, `-`, `@`, or a tab/CR that some programs strip before looking) gets a leading
/// apostrophe, which spreadsheets show as plain text.
///
/// Plain numbers such as the negative dBm values (`-62`) are left alone: they cannot carry a
/// formula, and prefixing them would turn numeric columns into text.
pub fn neutralize_formula(cell: &str) -> Cow<'_, str> {
    let risky = cell.starts_with(['=', '+', '-', '@', '\t', '\r']);
    if risky && !is_plain_number(cell) { Cow::Owned(format!("'{cell}")) } else { Cow::Borrowed(cell) }
}

/// Optional sign, digits, at most one decimal point, and nothing else (so "inf", "1e5",
/// "-1+2" and anything with letters or operators do not qualify).
fn is_plain_number(s: &str) -> bool {
    let body = s.strip_prefix(['-', '+']).unwrap_or(s);
    let mut dots = 0;
    let mut digits = 0;
    for ch in body.chars() {
        match ch {
            '0'..='9' => digits += 1,
            '.' => dots += 1,
            _ => return false,
        }
    }
    digits > 0 && dots <= 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// Malformed quoting is an error.
    Strict,
    /// Malformed quoting is tolerated: a stray quote is literal text and an unterminated
    /// quoted field runs to the end of the input.
    Lenient,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CsvError {
    #[error("line {line}: unterminated quoted field")]
    Unterminated { line: usize },
    #[error("line {line}: unexpected quote inside an unquoted field")]
    StrayQuote { line: usize },
    #[error("line {line}: text after the closing quote")]
    TrailingText { line: usize },
}

/// A parsed record and the 1-based input line it starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub line: usize,
    pub fields: Vec<String>,
}

#[derive(PartialEq, Eq)]
enum State {
    FieldStart,
    Unquoted,
    Quoted,
    /// Just saw a quote while inside a quoted field: either an escaped quote or the closing one.
    QuoteInQuoted,
    AfterQuoted,
}

/// Parses `text` into records. A leading UTF-8 BOM is skipped, LF/CRLF/CR all end records,
/// quoted fields may contain delimiters and line breaks, and completely blank lines are skipped.
pub fn parse(text: &str, delimiter: char, mode: ParseMode) -> Result<Vec<Record>, CsvError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut state = State::FieldStart;
    let mut line = 1;
    let mut record_line = 1;
    // True once the current record holds anything, including an empty quoted field `""`.
    let mut started = false;

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let is_break = ch == '\n' || ch == '\r';
        let crlf = ch == '\r' && chars.peek() == Some(&'\n');
        if crlf {
            chars.next();
        }
        match state {
            State::Quoted => {
                if ch == '"' {
                    state = State::QuoteInQuoted;
                } else {
                    field.push(ch);
                    if crlf {
                        field.push('\n');
                    }
                    if is_break {
                        line += 1;
                    }
                }
                continue;
            }
            State::QuoteInQuoted if ch == '"' => {
                field.push('"');
                state = State::Quoted;
                continue;
            }
            State::QuoteInQuoted => state = State::AfterQuoted,
            _ => {}
        }

        if is_break {
            if started || !field.is_empty() || !fields.is_empty() {
                fields.push(std::mem::take(&mut field));
                records.push(Record { line: record_line, fields: std::mem::take(&mut fields) });
            }
            started = false;
            state = State::FieldStart;
            line += 1;
            record_line = line;
        } else if ch == delimiter {
            fields.push(std::mem::take(&mut field));
            started = true;
            state = State::FieldStart;
        } else {
            match (&state, ch) {
                (State::FieldStart, '"') => {
                    state = State::Quoted;
                    started = true;
                }
                (State::AfterQuoted, _) => {
                    if mode == ParseMode::Strict {
                        return Err(CsvError::TrailingText { line });
                    }
                    field.push(ch);
                }
                (State::Unquoted, '"') => {
                    if mode == ParseMode::Strict {
                        return Err(CsvError::StrayQuote { line });
                    }
                    field.push(ch);
                }
                _ => {
                    field.push(ch);
                    started = true;
                    state = State::Unquoted;
                }
            }
        }
    }

    if state == State::Quoted && mode == ParseMode::Strict {
        return Err(CsvError::Unterminated { line: record_line });
    }
    if started || !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        records.push(Record { line: record_line, fields });
    }
    Ok(records)
}

/// Guesses the delimiter from the first non-empty line, counting only characters outside
/// quotes. Falls back to a comma.
pub fn detect_delimiter(text: &str) -> char {
    let first = text.trim_start_matches('\u{feff}').lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut counts = [(',', 0usize), (';', 0), ('\t', 0)];
    let mut in_quotes = false;
    for ch in first.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if let Some(entry) = counts.iter_mut().find(|(d, _)| *d == ch) {
                entry.1 += 1;
            }
        }
    }
    // `max_by_key` keeps the last maximum, so iterate in reverse to prefer the comma on ties.
    counts.iter().rev().max_by_key(|(_, n)| *n).filter(|(_, n)| *n > 0).map_or(',', |(d, _)| *d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(text: &str, mode: ParseMode) -> Vec<Vec<String>> {
        parse(text, ',', mode).unwrap().into_iter().map(|r| r.fields).collect()
    }

    fn row(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn escape_quotes_only_when_needed() {
        assert_eq!(escape_field("plain", ','), "plain");
        assert_eq!(escape_field("a,b", ','), "\"a,b\"");
        assert_eq!(escape_field("say \"hi\"", ','), "\"say \"\"hi\"\"\"");
        assert_eq!(escape_field("two\nlines", ','), "\"two\nlines\"");
        assert_eq!(escape_field("cr\rhere", ','), "\"cr\rhere\"");
        assert_eq!(escape_field("a;b", ','), "a;b");
        assert_eq!(escape_field("a;b", ';'), "\"a;b\"");
    }

    #[test]
    fn record_is_crlf_terminated() {
        assert_eq!(write_record(&["a", "b,c"], ','), "a,\"b,c\"\r\n");
    }

    #[test]
    fn formula_guard_prefixes_dangerous_cells_but_not_numbers() {
        for bad in ["=1+1", "+cmd|' /C calc'!A0", "-2+3", "@SUM(A1)", "=HYPERLINK(\"x\")", "\t=1", "-abc"] {
            assert_eq!(neutralize_formula(bad), format!("'{bad}"), "{bad}");
        }
        for ok in ["-62", "+5", "-62.5", "hello", "a=b", "", "1-2", "'=already"] {
            assert_eq!(neutralize_formula(ok), ok, "{ok}");
        }
        // Not plain numbers, so they stay guarded.
        for bad in ["-1e5", "-inf", "-", "-.", "--1", "-1.2.3"] {
            assert_eq!(neutralize_formula(bad), format!("'{bad}"), "{bad}");
        }
    }

    #[test]
    fn parses_simple_quoted_and_empty_fields() {
        assert_eq!(fields("a,b,c\n1,,3", ParseMode::Strict), vec![row(&["a", "b", "c"]), row(&["1", "", "3"])]);
        assert_eq!(fields("\"a,b\",\"say \"\"hi\"\"\"", ParseMode::Strict), vec![row(&["a,b", "say \"hi\""])]);
        assert_eq!(fields("\"\",x", ParseMode::Strict), vec![row(&["", "x"])]);
        assert_eq!(fields("a,", ParseMode::Strict), vec![row(&["a", ""])]);
    }

    #[test]
    fn handles_line_endings_bom_and_blank_lines() {
        let text = "\u{feff}a,b\r\n\r\nc,d\re,f\n\n";
        assert_eq!(fields(text, ParseMode::Strict), vec![row(&["a", "b"]), row(&["c", "d"]), row(&["e", "f"])]);
    }

    #[test]
    fn quoted_fields_keep_embedded_newlines_and_line_numbers_count_them() {
        let recs = parse("x,\"one\ntwo\"\nnext,y", ',', ParseMode::Strict).unwrap();
        assert_eq!(recs[0].fields, row(&["x", "one\ntwo"]));
        assert_eq!(recs[0].line, 1);
        assert_eq!(recs[1].line, 3, "the embedded newline counts as a line");
    }

    #[test]
    fn strict_rejects_malformed_quoting_with_the_line() {
        assert_eq!(parse("ok,1\nbad,\"open", ',', ParseMode::Strict), Err(CsvError::Unterminated { line: 2 }));
        assert_eq!(parse("a\nb\"c,d", ',', ParseMode::Strict), Err(CsvError::StrayQuote { line: 2 }));
        assert_eq!(parse("\"a\"x,b", ',', ParseMode::Strict), Err(CsvError::TrailingText { line: 1 }));
    }

    #[test]
    fn lenient_tolerates_malformed_quoting() {
        assert_eq!(fields("b\"c,d", ParseMode::Lenient), vec![row(&["b\"c", "d"])]);
        assert_eq!(fields("\"a\"x,b", ParseMode::Lenient), vec![row(&["ax", "b"])]);
        assert_eq!(fields("k,\"open ended\nstill", ParseMode::Lenient), vec![row(&["k", "open ended\nstill"])]);
    }

    #[test]
    fn write_then_parse_round_trips_awkward_values() {
        let original = ["plain", "with,comma", "with \"quotes\"", "multi\nline", "", " padded "];
        let text = write_record(&original, ',');
        let parsed = parse(&text, ',', ParseMode::Strict).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].fields, row(&original));
    }

    #[test]
    fn quoted_crlf_is_preserved() {
        assert_eq!(fields("\"a\r\nb\",c", ParseMode::Strict), vec![row(&["a\r\nb", "c"])]);
    }

    #[test]
    fn a_single_empty_quoted_field_is_a_record() {
        assert_eq!(fields("\"\"\n", ParseMode::Strict), vec![row(&[""])]);
    }

    #[test]
    fn delimiter_detection_ignores_quoted_text_and_defaults_to_comma() {
        assert_eq!(detect_delimiter("BSSID;alias\nAA;x"), ';');
        assert_eq!(detect_delimiter("BSSID\talias"), '\t');
        assert_eq!(detect_delimiter("\"a;b\",c"), ',');
        assert_eq!(detect_delimiter("single"), ',');
        assert_eq!(detect_delimiter(""), ',');
        assert_eq!(detect_delimiter("a,b;c"), ',', "ties prefer the comma");
    }
}
