//! CSV of a database (spec P2 9.2), with the same rules as `@orbit/database/csv.ts`: RFC 4180,
//! UTF-8 with a BOM, CRLF line ends, a header row of property names, and a leading `'` on text
//! cells a spreadsheet app would run as a formula.

use std::collections::HashMap;

use orbit_platform::Id;

use crate::repositories::databases::{ExportDatabase, Property, cell_text, is_number};

/// `text` with a leading `'` when it starts with `=`, `+`, `-`, `@`, tab or CR and is not a number.
#[must_use]
pub fn guard_cell(text: &str, number: bool) -> String {
    if !number && text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    }
}

/// Records as CSV text; a field is quoted when it holds `"`, `,`, `;` or a line break.
#[must_use]
pub fn to_csv(records: &[Vec<String>]) -> String {
    let field = |text: &str| {
        if text.contains(['"', ',', ';', '\r', '\n']) {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.to_owned()
        }
    };
    let mut out = String::from('\u{feff}');
    for record in records {
        let line: Vec<String> = record.iter().map(|text| field(text)).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

/// `properties` of every row of the database, as CSV text.
#[must_use]
pub fn database_csv(
    database: &ExportDatabase,
    properties: &[&Property],
    members: &HashMap<Id, String>,
) -> String {
    let mut records = Vec::with_capacity(database.rows.len() + 1);
    records.push(
        properties
            .iter()
            .map(|property| guard_cell(&property.name, false))
            .collect(),
    );
    for row in &database.rows {
        records.push(
            properties
                .iter()
                .map(|property| guard_cell(&cell_text(property, row, members), is_number(property)))
                .collect(),
        );
    }
    to_csv(&records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_formulas_but_not_numbers() {
        let cells: Vec<String> = ["=1+1", "+1", "-x", "@A1", "\tx", "\rx", "ok"]
            .iter()
            .map(|text| guard_cell(text, false))
            .collect();
        assert_eq!(cells, ["'=1+1", "'+1", "'-x", "'@A1", "'\tx", "'\rx", "ok"]);
        assert_eq!(guard_cell("-2.5", true), "-2.5");
    }

    #[test]
    fn quotes_fields_with_separators_and_breaks() {
        let records = vec![
            vec!["a".to_owned(), "b,c".to_owned()],
            vec!["say \"hi\"".to_owned(), "line\nbreak".to_owned()],
        ];
        assert_eq!(
            to_csv(&records),
            "\u{feff}a,\"b,c\"\r\n\"say \"\"hi\"\"\",\"line\nbreak\"\r\n"
        );
    }
}
