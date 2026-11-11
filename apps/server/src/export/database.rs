//! Markdown of a database (P1 9): the database page shows the rows of a view as a table whose
//! titles link to the row pages, and each row page starts with a list of its property values.

use std::collections::HashMap;

use orbit_platform::Id;

use super::markdown::{escape_cell, escape_label, link_destination};
use crate::repositories::databases::{
    DatabaseView, ExportDatabase, ExportRow, PropertyKind, cell_text,
};

/// The rows of `database` under the properties `view` shows. `row_link` gives the destination of
/// a row's page.
#[must_use]
pub fn markdown_table(
    database: &ExportDatabase,
    view: &DatabaseView,
    members: &HashMap<Id, String>,
    row_link: &dyn Fn(Id) -> String,
) -> String {
    let properties = database.view_properties(view);
    if properties.is_empty() {
        return String::new();
    }
    let header: Vec<String> = properties
        .iter()
        .map(|property| escape_cell(&property.name))
        .collect();
    let mut lines = vec![
        format!("| {} |", header.join(" | ")),
        format!("|{}", " --- |".repeat(properties.len())),
    ];
    for row in &database.rows {
        let cells: Vec<String> = properties
            .iter()
            .map(|property| {
                if property.kind == PropertyKind::Title {
                    let title = row.title.trim();
                    let label = if title.is_empty() { "Untitled" } else { title };
                    format!(
                        "[{}]({})",
                        escape_cell(label),
                        link_destination(&row_link(row.id))
                    )
                } else {
                    escape_cell(&cell_text(property, row, members))
                }
            })
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    lines.join("\n")
}

/// The row's values as a list, one line per property in property order (the title and empty
/// values are left out).
#[must_use]
pub fn property_list(
    database: &ExportDatabase,
    row: &ExportRow,
    members: &HashMap<Id, String>,
) -> String {
    database
        .properties
        .iter()
        .filter(|property| property.kind != PropertyKind::Title)
        .filter_map(|property| {
            let text = cell_text(property, row, members);
            let text = text.replace(['\r', '\n'], " ");
            let text = text.trim();
            (!text.is_empty()).then(|| {
                format!(
                    "- **{}:** {}",
                    escape_label(&property.name),
                    escape_label(text)
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}
