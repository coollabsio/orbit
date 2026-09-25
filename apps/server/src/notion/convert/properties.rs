//! Database row properties → a two-column BlockNote table (property name | value).

use serde_json::{Number, Value};

use super::rich_text::{
    format_date_value, format_timestamp, link, plain, resolve_href, rich_text_to_inline, str_at,
};
use super::{ConvertContext, make_block, table_cell, table_content, table_props};
use crate::notion::model::{NotionPage, normalize_id};

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
    for (name, value) in entries {
        let Some(content) = property_inline(value, ctx) else {
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

/// A property value as inline content; `None` for empty or unsupported values.
fn property_inline(value: &Value, ctx: &mut ConvertContext<'_>) -> Option<Vec<Value>> {
    let kind = str_at(value, &["type"])?;
    let data = value.get(kind).unwrap_or(&Value::Null);
    let text = |value: String| (!value.trim().is_empty()).then(|| vec![plain(value)]);
    match kind {
        "title" | "rich_text" => Some(rich_text_to_inline(data.as_array()?, ctx)),
        "number" => text(format_number(data.as_number()?)),
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
                        let Some(content) = property_inline(item, ctx) else {
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
