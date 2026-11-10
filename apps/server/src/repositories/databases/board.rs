//! Board grouping (spec P2 8): the property whose groups are the columns, the optional property
//! whose groups are the lanes, and the group ids each of them has.

use std::collections::HashSet;

use orbit_platform::{Id, Row};
use sqlx::{Any, Transaction};

use super::{DatabaseError, MAX_OPTIONS, Property, PropertyKind, ViewConfig, ViewType};

/// A property whose groups can be a board's columns.
pub(super) const fn groupable(kind: &PropertyKind) -> bool {
    matches!(
        kind,
        PropertyKind::Select(_)
            | PropertyKind::Status(_)
            | PropertyKind::MultiSelect(_)
            | PropertyKind::Person
    )
}

/// A property whose groups can be a board's lanes: a column property or a checkbox.
const fn laneable(kind: &PropertyKind) -> bool {
    groupable(kind) || matches!(kind, PropertyKind::Checkbox)
}

/// Whether `group` is a group of a property of `kind`: an option id or a user id, `null` for "no
/// value", and `"true"` or `"false"` for a checkbox (which has no empty group).
fn fits(kind: &PropertyKind, group: Option<&str>) -> bool {
    match (kind, group) {
        (PropertyKind::Checkbox, group) => matches!(group, Some("true" | "false")),
        (_, None) => true,
        (PropertyKind::Person, Some(id)) => id.parse::<Id>().is_ok(),
        (kind, Some(id)) => kind.has_option(id),
    }
}

/// Drops groups that are not groups of `kind`, and repeats.
fn retain_groups(groups: &mut Vec<Option<String>>, kind: Option<&PropertyKind>) {
    let mut seen = HashSet::new();
    groups.retain(|group| {
        kind.is_some_and(|kind| fits(kind, group.as_deref())) && seen.insert(group.clone())
    });
}

/// The property a board groups by when it names none: the first status, else the first select.
fn default_group_by(properties: &[Property]) -> Option<Id> {
    properties
        .iter()
        .find(|property| matches!(property.kind, PropertyKind::Status(_)))
        .or_else(|| {
            properties
                .iter()
                .find(|property| matches!(property.kind, PropertyKind::Select(_)))
        })
        .map(|property| property.id)
}

/// The board part of `ViewConfig::repair`. A board whose group property is gone or cannot group
/// groups by the default one, with the default order; a lane property that is gone, cannot group
/// lanes or is the group property is dropped with its order. Groups that are not groups of their
/// property go.
pub(super) fn repair(config: &mut ViewConfig, view_type: ViewType, properties: &[Property]) {
    let kind = |id: Id| {
        properties
            .iter()
            .find(|property| property.id == id)
            .map(|property| &property.kind)
    };
    let group_by = config
        .group_by
        .filter(|id| kind(*id).is_some_and(groupable));
    let group_by = match view_type {
        ViewType::Board => group_by.or_else(|| default_group_by(properties)),
        ViewType::Table => group_by,
    };
    if group_by != config.group_by {
        config.group_order.clear();
        config.hidden_groups.clear();
        config.group_by = group_by;
    }
    let sub_group_by = config
        .sub_group_by
        .filter(|id| Some(*id) != group_by && kind(*id).is_some_and(laneable));
    if sub_group_by != config.sub_group_by {
        config.sub_group_order.clear();
        config.hidden_sub_groups.clear();
        config.sub_group_by = sub_group_by;
    }
    let group_kind = group_by.and_then(kind);
    retain_groups(&mut config.group_order, group_kind);
    retain_groups(&mut config.hidden_groups, group_kind);
    let lane_kind = sub_group_by.and_then(kind);
    retain_groups(&mut config.sub_group_order, lane_kind);
    retain_groups(&mut config.hidden_sub_groups, lane_kind);
}

/// Checks the board fields a client sends; errors name the field path.
pub(super) fn check(config: &ViewConfig, properties: &[Property]) -> Result<(), String> {
    for (field, groups) in [
        ("group_order", &config.group_order),
        ("hidden_groups", &config.hidden_groups),
        ("sub_group_order", &config.sub_group_order),
        ("hidden_sub_groups", &config.hidden_sub_groups),
    ] {
        if groups.len() > MAX_OPTIONS + 1 {
            return Err(format!("config.{field}"));
        }
    }
    let kind = |id: Option<Id>| {
        properties
            .iter()
            .find(|property| Some(property.id) == id)
            .map(|property| &property.kind)
    };
    if kind(config.group_by).is_some_and(|kind| !groupable(kind)) {
        return Err("config.group_by".to_owned());
    }
    if config.sub_group_by.is_some()
        && (config.sub_group_by == config.group_by
            || kind(config.sub_group_by).is_some_and(|kind| !laneable(kind)))
    {
        return Err("config.sub_group_by".to_owned());
    }
    Ok(())
}

/// Drops the groups of person properties that name a user who is no longer a member of the
/// database's workspace. Reads the members only when a config groups by a person.
pub(super) async fn drop_former_members(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    properties: &[Property],
    configs: &mut [&mut ViewConfig],
) -> Result<(), DatabaseError> {
    let people: HashSet<Id> = properties
        .iter()
        .filter(|property| property.kind == PropertyKind::Person)
        .map(|property| property.id)
        .collect();
    let by_person = |id: Option<Id>| id.is_some_and(|id| people.contains(&id));
    if !configs
        .iter()
        .any(|config| by_person(config.group_by) || by_person(config.sub_group_by))
    {
        return Ok(());
    }
    let members: HashSet<String> = sqlx::query(
        "SELECT memberships.user_id AS user_id FROM memberships \
         JOIN pages ON pages.workspace_id = memberships.workspace_id \
         WHERE pages.id = $1 AND memberships.role <> 'guest'",
    )
    .bind(database_id.to_string())
    .fetch_all(&mut **tx)
    .await?
    .iter()
    .map(|row| row.get("user_id"))
    .collect();
    let keep = |groups: &mut Vec<Option<String>>| {
        groups.retain(|group| group.as_ref().is_none_or(|id| members.contains(id)));
    };
    for config in configs {
        if by_person(config.group_by) {
            keep(&mut config.group_order);
            keep(&mut config.hidden_groups);
        }
        if by_person(config.sub_group_by) {
            keep(&mut config.sub_group_order);
            keep(&mut config.hidden_sub_groups);
        }
    }
    Ok(())
}
