//! Database row properties → a two-column BlockNote table (property name | value).

use serde_json::{Number, Value};

use super::rich_text::{
    format_date_value, format_timestamp, link, plain, resolve_href, rich_text_to_inline, str_at,
};
use super::{ConvertContext, Resolver, make_block, table_cell, table_content, table_props};
use crate::notion::model::{NotionPage, Parent, normalize_id};

/// Renders a database row's properties as a BlockNote table block (name column as header
/// column), skipping empty values. The title comes first, then the other properties by name
/// (property order is not preserved by the API objects). Returns `None` when every value is
/// empty.
///
/// Values: text as rich text; number; select/status name; multi-select names; date (range);
/// people names; checkbox Yes/No; url/email links; phone; relations as `/docs` links when
/// imported, else a count; formula/rollup results; file names; created/edited time and user;
/// unique id; place; verification state.
pub fn database_row_properties_table(
    page: &NotionPage,
    ctx: &mut ConvertContext<'_>,
) -> Option<Value> {
    let mut entries: Vec<(&String, &Value)> = page.properties.iter().collect();
    entries.sort_by_key(|(name, value)| (str_at(value, &["type"]) != Some("title"), name.as_str()));
    let mut rows = Vec::new();
    let resolver = ctx.resolver;
    for (name, value) in entries {
        let format = number_format(page, name, resolver);
        let Some(content) = property_inline(value, format, ctx) else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        rows.push(serde_json::json!({
            "cells": [table_cell(vec![plain(name.clone())]), table_cell(content)]
        }));
    }
    if rows.is_empty() {
        return None;
    }
    let content = table_content(2, false, true, rows);
    Some(make_block(
        ctx,
        "table",
        table_props(),
        Some(content),
        Vec::new(),
    ))
}

/// Rows shown in one table; more are counted as `database_rows_truncated` (every row stays a
/// sub-page).
const MAX_TABLE_ROWS: usize = 1_000;

/// Renders a database's rows as a BlockNote table, like Notion's table view: a header row with
/// the property names (the title first, then the others by name), and one row per page whose
/// title links to the imported page. `None` without rows.
pub fn database_table(rows: &[NotionPage], ctx: &mut ConvertContext<'_>) -> Option<Value> {
    let rows: Vec<&NotionPage> = rows.iter().filter(|row| !row.in_trash).collect();
    if rows.is_empty() {
        return None;
    }
    let mut columns: Vec<(&String, bool)> = Vec::new();
    for row in &rows {
        for (name, value) in &row.properties {
            let kind = str_at(value, &["type"]);
            if kind != Some("button") && !columns.iter().any(|(known, _)| *known == name) {
                columns.push((name, kind == Some("title")));
            }
        }
    }
    columns.sort_by_key(|(name, title)| (!title, name.as_str()));
    let header = columns
        .iter()
        .map(|(name, _)| table_cell(vec![plain((*name).clone())]))
        .collect();
    let mut out = vec![serde_json::json!({ "cells": Value::Array(header) })];
    if rows.len() > MAX_TABLE_ROWS {
        ctx.lossy("database_rows_truncated");
    }
    let resolver = ctx.resolver;
    for row in rows.into_iter().take(MAX_TABLE_ROWS) {
        let cells = columns
            .iter()
            .map(|(name, title)| {
                let value = row.properties.get(*name);
                let content = if *title {
                    title_cell(row, value, ctx)
                } else {
                    let format = number_format(row, name, resolver);
                    value.and_then(|value| property_inline(value, format, ctx))
                };
                table_cell(content.unwrap_or_default())
            })
            .collect();
        out.push(serde_json::json!({ "cells": Value::Array(cells) }));
    }
    let content = table_content(columns.len(), true, false, out);
    Some(make_block(
        ctx,
        "table",
        table_props(),
        Some(content),
        Vec::new(),
    ))
}

/// The row's title, linked to its imported page.
fn title_cell(
    row: &NotionPage,
    value: Option<&Value>,
    ctx: &mut ConvertContext<'_>,
) -> Option<Vec<Value>> {
    let title = value
        .and_then(|value| value.get("title"))
        .and_then(Value::as_array)
        .map(|items| crate::notion::model::plain_text(items))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_owned());
    let page = normalize_id(&row.id).and_then(|id| ctx.resolver.page_link(&id));
    Some(match page {
        Some(page) => vec![link(format!("/docs/{page}"), vec![plain(title)])],
        None => vec![plain(title)],
    })
}

/// The number format of a row's property, from its data source's schema.
fn number_format<'r>(
    row: &NotionPage,
    property: &str,
    resolver: &'r dyn Resolver,
) -> Option<&'r str> {
    let Some(Parent::DataSourceId { data_source_id, .. }) = &row.parent else {
        return None;
    };
    resolver.number_format(&normalize_id(data_source_id)?, property)
}

/// A property value as inline content; `None` for empty or unsupported values. `format` is the
/// property's number format.
fn property_inline(
    value: &Value,
    format: Option<&str>,
    ctx: &mut ConvertContext<'_>,
) -> Option<Vec<Value>> {
    let kind = str_at(value, &["type"])?;
    let data = value.get(kind).unwrap_or(&Value::Null);
    let text = |value: String| (!value.trim().is_empty()).then(|| vec![plain(value)]);
    match kind {
        "title" | "rich_text" => Some(rich_text_to_inline(data.as_array()?, ctx)),
        "number" => text(format_number_as(data.as_number()?, format)),
        "select" | "status" => text(str_at(data, &["name"])?.to_owned()),
        "multi_select" => {
            text(join(data.as_array()?.iter().filter_map(|option| {
                str_at(option, &["name"]).map(str::to_owned)
            })))
        }
        "date" => text(format_date_value(data)?),
        "people" => text(join(
            data.as_array()?.iter().map(|user| user_name(user, ctx)),
        )),
        "checkbox" => text(if data.as_bool()? { "Yes" } else { "No" }.to_owned()),
        "url" => url_link(data.as_str()?, data.as_str()?, ctx),
        "email" => {
            let email = data.as_str()?.trim();
            url_link(&format!("mailto:{email}"), email, ctx)
        }
        "phone_number" => text(data.as_str()?.to_owned()),
        "relation" => relation(value, data.as_array()?, ctx),
        "formula" => {
            let result_kind = str_at(data, &["type"])?;
            let result = data.get(result_kind)?;
            match result_kind {
                "string" => text(result.as_str()?.to_owned()),
                "number" => text(format_number(result.as_number()?)),
                "boolean" => text(if result.as_bool()? { "Yes" } else { "No" }.to_owned()),
                "date" => text(format_date_value(result)?),
                _ => None,
            }
        }
        "rollup" => {
            let result_kind = str_at(data, &["type"])?;
            let result = data.get(result_kind)?;
            match result_kind {
                "number" => text(format_number(result.as_number()?)),
                "date" => text(format_date_value(result)?),
                "array" => {
                    let mut out = Vec::new();
                    for item in result.as_array()? {
                        let Some(content) = property_inline(item, None, ctx) else {
                            continue;
                        };
                        if content.is_empty() {
                            continue;
                        }
                        if !out.is_empty() {
                            out.push(plain(", "));
                        }
                        out.extend(content);
                    }
                    Some(out)
                }
                _ => None,
            }
        }
        "files" => text(join(data.as_array()?.iter().filter_map(|file| {
            str_at(file, &["name"])
                .map(str::to_owned)
                .or_else(|| str_at(file, &["external", "url"]).map(str::to_owned))
        }))),
        "created_time" | "last_edited_time" => text(format_timestamp(data.as_str()?)),
        "created_by" | "last_edited_by" => text(user_name(data, ctx)),
        "unique_id" => {
            let number = data.get("number")?.as_number().map(format_number)?;
            match str_at(data, &["prefix"]) {
                Some(prefix) if !prefix.is_empty() => text(format!("{prefix}-{number}")),
                _ => text(number),
            }
        }
        "place" => {
            let label = str_at(data, &["name"])
                .or_else(|| str_at(data, &["address"]))
                .map(str::to_owned)
                .or_else(|| {
                    let lat = data.get("lat")?.as_number()?;
                    let lon = data.get("lon")?.as_number()?;
                    Some(format!("{lat}, {lon}"))
                });
            text(label?)
        }
        "verification" => {
            let state = str_at(data, &["state"])?;
            match data.get("date").and_then(|date| str_at(date, &["end"])) {
                Some(end) if state == "verified" => {
                    text(format!("verified until {}", format_timestamp(end)))
                }
                _ => text(state.to_owned()),
            }
        }
        "button" => None,
        other => {
            ctx.lossy(&format!("property:{other}"));
            None
        }
    }
}

fn url_link(href: &str, label: &str, ctx: &mut ConvertContext<'_>) -> Option<Vec<Value>> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    Some(match resolve_href(href, ctx) {
        Some(href) => vec![link(href, vec![plain(label)])],
        None => vec![plain(label)],
    })
}

/// Imported related pages as `/docs` links, the rest as a count.
fn relation(value: &Value, related: &[Value], ctx: &mut ConvertContext<'_>) -> Option<Vec<Value>> {
    let mut out = Vec::new();
    let mut unresolved = 0u32;
    for id in related
        .iter()
        .filter_map(|item| str_at(item, &["id"]))
        .filter_map(normalize_id)
    {
        match ctx.resolver.page_link(&id) {
            Some(orbit_id) => {
                let title = ctx
                    .resolver
                    .page_title(&id)
                    .unwrap_or_else(|| "Linked page".to_owned());
                if !out.is_empty() {
                    out.push(plain(", "));
                }
                out.push(link(format!("/docs/{orbit_id}"), vec![plain(title)]));
            }
            None => unresolved += 1,
        }
    }
    ctx.report.unresolved_page_links += unresolved;
    if unresolved > 0 {
        let separator = if out.is_empty() { "" } else { ", " };
        let noun = if unresolved == 1 { "page" } else { "pages" };
        let prefix = if out.is_empty() { "" } else { "+" };
        out.push(plain(format!(
            "{separator}{prefix}{unresolved} linked {noun}"
        )));
    }
    if value.get("has_more").and_then(Value::as_bool) == Some(true) {
        out.push(plain(if out.is_empty() {
            "More than 25 linked pages"
        } else {
            " and more"
        }));
    }
    Some(out)
}

fn user_name(user: &Value, ctx: &ConvertContext<'_>) -> String {
    str_at(user, &["name"])
        .map(str::to_owned)
        .or_else(|| str_at(user, &["id"]).and_then(|id| ctx.resolver.user_name(id)))
        .unwrap_or_else(|| "Unknown user".to_owned())
}

fn join(values: impl Iterator<Item = String>) -> String {
    values
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A number as Notion shows it in `format`: `number_with_commas` groups thousands, `percent`
/// multiplies by 100, and currencies get their symbol and two decimals (none for yen, won and
/// dong). Unknown formats show the plain number.
pub fn format_number_as(number: &Number, format: Option<&str>) -> String {
    let Some(value) = number.as_f64() else {
        return format_number(number);
    };
    let format = format.unwrap_or("number");
    if format == "number_with_commas" {
        return group_thousands(&format_number(number));
    }
    if format == "percent" {
        // Rounded so that 0.07 shows as 7%, not 7.000000000000001%.
        let percent = (value * 100.0 * 1e9).round() / 1e9;
        return format!("{}%", format_float(percent));
    }
    let Some(symbol) = currency_symbol(format) else {
        return format_number(number);
    };
    let digits = match format {
        "yen" | "won" | "vietnamese_dong" => format!("{:.0}", value.abs()),
        "bitcoin" => format_float(value.abs()),
        _ => format!("{:.2}", value.abs()),
    };
    let sign = if value < 0.0 { "-" } else { "" };
    format!("{sign}{symbol}{}", group_thousands(&digits))
}

fn currency_symbol(format: &str) -> Option<&'static str> {
    Some(match format {
        "dollar" => "$",
        "australian_dollar" => "A$",
        "canadian_dollar" => "CA$",
        "singapore_dollar" => "S$",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "ruble" => "₽",
        "rupee" => "₹",
        "won" => "₩",
        "yuan" => "CN¥",
        "real" => "R$",
        "lira" => "₺",
        "rupiah" => "Rp",
        "franc" => "CHF ",
        "hong_kong_dollar" => "HK$",
        "new_zealand_dollar" => "NZ$",
        "krona" => "SEK ",
        "norwegian_krone" => "NOK ",
        "mexican_peso" => "MX$",
        "rand" => "R",
        "new_taiwan_dollar" => "NT$",
        "danish_krone" => "DKK ",
        "zloty" => "zł",
        "baht" => "฿",
        "forint" => "Ft",
        "koruna" => "Kč",
        "shekel" => "₪",
        "chilean_peso" => "CLP$",
        "philippine_peso" => "₱",
        "dirham" => "AED ",
        "colombian_peso" => "COP$",
        "riyal" => "SAR ",
        "ringgit" => "RM",
        "leu" => "lei ",
        "argentine_peso" => "ARS$",
        "uruguayan_peso" => "$U",
        "peruvian_sol" => "S/",
        "vietnamese_dong" => "₫",
        "pakistani_rupee" => "Rs",
        "nigerian_naira" => "₦",
        "bitcoin" => "₿",
        _ => return None,
    })
}

/// `1234567.5` → `1,234,567.5` (keeps a leading `-`).
fn group_thousands(number: &str) -> String {
    let (sign, rest) = number
        .strip_prefix('-')
        .map_or(("", number), |rest| ("-", rest));
    let (int, frac) = rest
        .split_once('.')
        .map_or((rest, None), |(i, f)| (i, Some(f)));
    let mut grouped = String::new();
    for (index, digit) in int.chars().enumerate() {
        if index > 0 && (int.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match frac {
        Some(frac) => format!("{sign}{grouped}.{frac}"),
        None => format!("{sign}{grouped}"),
    }
}

fn format_float(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn format_number(number: &Number) -> String {
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    match number.as_f64() {
        Some(value) if value.fract() == 0.0 && value.abs() < 1e15 => format!("{}", value as i64),
        Some(value) => format!("{value}"),
        None => number.to_string(),
    }
}
