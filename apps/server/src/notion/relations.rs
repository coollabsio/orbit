//! Notion relations and rollups that stay live (spec P3 11.9). The plan makes every Notion
//! relation a text property of related titles and every rollup static values (`database.rs`).
//! While pass 1 creates the databases and rows, [`RelationPlan`] keeps each relation's schema
//! and each row's linked Notion pages; after pass 1, [`RelationPlan::apply`] turns relations
//! whose target database came with the import into Orbit relations (`dual_property` becomes a
//! two-way pair, `single_property` one-way), and then rollups over them into Orbit rollups.
//! Links to pages outside the import stay as text in "<name> (titles)". Rows created by an
//! earlier, interrupted run of the same import keep their text.

use std::collections::HashMap;

use orbit_platform::{Id, TimestampMillis};
use serde_json::Value;

use super::database::{Source, property_id};
use super::model::{NotionPage, normalize_id};
use crate::repositories::databases::{
    DatabaseError, DatabaseRepository, ImportedRelation, ImportedRollup, ImportedSide,
    RollupConfig, RollupFunction,
};

struct NotionRelation {
    database: Id,
    notion_id: String,
    name: String,
    /// Normalized Notion data source and database ids of the target.
    target_source: Option<String>,
    target_database: Option<String>,
    /// The other property of a `dual_property` pair, in the target.
    synced: Option<String>,
}

struct NotionRollup {
    database: Id,
    notion_id: String,
    relation: String,
    target: String,
    function: String,
}

/// What [`RelationPlan::apply`] made live, so the report stops counting it as lossy.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Converted {
    /// Planned title-text properties that went: every link of the side was inside.
    pub relation_titles: u32,
    /// Rollups that compute again instead of keeping static values.
    pub rollup_values: u32,
}

#[derive(Default)]
pub struct RelationPlan {
    relations: Vec<NotionRelation>,
    rollups: Vec<NotionRollup>,
    /// `(database, relation property id)` → rows with their linked Notion page ids.
    links: HashMap<(Id, Id), Vec<(Id, Vec<String>)>>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

impl RelationPlan {
    /// Keeps the relation and rollup schema of the data source that became `database`.
    pub fn add_schema(&mut self, database: Id, source: &Source) {
        for (name, schema) in &source.data_source.properties {
            let Some(notion_id) = text(schema, "id") else {
                continue;
            };
            match schema.get("type").and_then(Value::as_str) {
                Some("relation") => {
                    let config = schema.get("relation").unwrap_or(&Value::Null);
                    let dual = config.get("type").and_then(Value::as_str) == Some("dual_property");
                    self.relations.push(NotionRelation {
                        database,
                        notion_id,
                        name: name.clone(),
                        target_source: text(config, "data_source_id")
                            .as_deref()
                            .and_then(normalize_id),
                        target_database: text(config, "database_id")
                            .as_deref()
                            .and_then(normalize_id),
                        synced: dual
                            .then(|| config.get("dual_property"))
                            .flatten()
                            .and_then(|dual| text(dual, "synced_property_id")),
                    });
                }
                Some("rollup") => {
                    let config = schema.get("rollup").unwrap_or(&Value::Null);
                    if let (Some(relation), Some(target), Some(function)) = (
                        text(config, "relation_property_id"),
                        text(config, "rollup_property_id"),
                        text(config, "function"),
                    ) {
                        self.rollups.push(NotionRollup {
                            database,
                            notion_id,
                            relation,
                            target,
                            function,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// Keeps the linked pages of a row (`row_id` in `database`), after its relation lists were
    /// read in full.
    pub fn add_row(&mut self, database: Id, row_id: Id, row: &NotionPage) {
        for value in row.properties.values() {
            if value.get("type").and_then(Value::as_str) != Some("relation") {
                continue;
            }
            let Some(notion_id) = text(value, "id") else {
                continue;
            };
            let pages: Vec<String> = value
                .get("relation")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|page| page.get("id").and_then(Value::as_str))
                .filter_map(normalize_id)
                .collect();
            self.links
                .entry((database, property_id(database, &notion_id)))
                .or_default()
                .push((row_id, pages));
        }
    }

    /// Converts what can stay live. `pages`: normalized Notion id → Orbit page id of every
    /// page the import created (databases by data source and database id); `rows`: normalized
    /// Notion id of every created row → its Orbit database; `titles`: Notion titles for links
    /// outside the import. A link is inside when it names a row of the relation's target.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply(
        &self,
        databases: &DatabaseRepository,
        workspace_id: Id,
        actor_id: Id,
        pages: &HashMap<String, Id>,
        rows: &HashMap<String, Id>,
        titles: &HashMap<String, String>,
        request_id: &str,
    ) -> Result<Converted, DatabaseError> {
        let mut converted = Converted::default();
        let target_of = |relation: &NotionRelation| {
            relation
                .target_source
                .as_ref()
                .and_then(|id| pages.get(id))
                .or_else(|| {
                    relation
                        .target_database
                        .as_ref()
                        .and_then(|id| pages.get(id))
                })
                .copied()
        };
        let key = |relation: &NotionRelation| property_id(relation.database, &relation.notion_id);
        // Relation property (planned id) → the live relation that replaced it.
        let mut live: HashMap<Id, Id> = HashMap::new();
        for relation in &self.relations {
            let Some(target) = target_of(relation) else {
                continue;
            };
            let partner = relation.synced.as_ref().and_then(|synced| {
                let synced = property_id(target, synced);
                self.relations.iter().find(|other| {
                    other.database == target
                        && key(other) == synced
                        && target_of(other) == Some(relation.database)
                        && key(other) != key(relation)
                })
            });
            // A pair is made once, from the side with the smaller property id.
            if partner.is_some_and(|partner| key(partner) < key(relation)) {
                continue;
            }
            let links = self
                .links
                .get(&(relation.database, key(relation)))
                .map_or(&[][..], Vec::as_slice);
            let inside = |page: &String, target: Id| rows.get(page) == Some(&target);
            let side = |relation: &NotionRelation, linked: &[(Id, Vec<String>)], target: Id| {
                ImportedSide {
                    database_id: relation.database,
                    text_property_id: key(relation),
                    relation_id: property_id(
                        relation.database,
                        &format!("{}#relation", relation.notion_id),
                    ),
                    name: relation.name.clone(),
                    outside: linked
                        .iter()
                        .map(|(row, linked)| {
                            let outside: Vec<&str> = linked
                                .iter()
                                .filter(|page| !inside(page, target))
                                .map(|page| titles.get(page).map_or("Untitled", String::as_str))
                                .collect();
                            (*row, (!outside.is_empty()).then(|| outside.join(", ")))
                        })
                        .collect(),
                }
            };
            let primary = side(relation, links, target);
            let synced = partner.map(|partner| {
                let linked = self
                    .links
                    .get(&(partner.database, key(partner)))
                    .map_or(&[][..], Vec::as_slice);
                side(partner, linked, relation.database)
            });
            let links = links
                .iter()
                .map(|(row, linked)| {
                    let mut ids: Vec<Id> = Vec::new();
                    let kept = linked.iter().filter(|page| inside(page, target));
                    for id in kept.filter_map(|page| pages.get(page)) {
                        if !ids.contains(id) {
                            ids.push(*id);
                        }
                    }
                    (*row, ids)
                })
                .collect();
            let ids = (
                primary.relation_id,
                synced
                    .as_ref()
                    .map(|side| (side.text_property_id, side.relation_id)),
            );
            let all_inside =
                |side: &ImportedSide| side.outside.iter().all(|(_, text)| text.is_none());
            let titles_gone = u32::from(all_inside(&primary))
                + synced
                    .as_ref()
                    .map_or(0, |side| u32::from(all_inside(side)));
            let made = databases
                .import_relation(
                    workspace_id,
                    actor_id,
                    ImportedRelation {
                        primary,
                        synced,
                        target_id: target,
                        links,
                    },
                    request_id,
                    TimestampMillis::now(),
                )
                .await?;
            if made {
                converted.relation_titles += titles_gone;
            }
            live.insert(key(relation), ids.0);
            if let Some((text, id)) = ids.1 {
                live.insert(text, id);
            }
        }
        for rollup in &self.rollups {
            let Some(relation) = self.relations.iter().find(|relation| {
                relation.database == rollup.database
                    && key(relation) == property_id(rollup.database, &rollup.relation)
            }) else {
                continue;
            };
            let (Some(relation_id), Some(target)) = (live.get(&key(relation)), target_of(relation))
            else {
                continue;
            };
            let Some(function) = rollup_function(&rollup.function) else {
                continue;
            };
            let target_property = property_id(target, &rollup.target);
            let made = databases
                .import_rollup(
                    workspace_id,
                    actor_id,
                    ImportedRollup {
                        database_id: rollup.database,
                        static_property_id: property_id(rollup.database, &rollup.notion_id),
                        rollup_id: property_id(
                            rollup.database,
                            &format!("{}#rollup", rollup.notion_id),
                        ),
                        config: RollupConfig {
                            relation_property_id: *relation_id,
                            target_property_id: live
                                .get(&target_property)
                                .copied()
                                .unwrap_or(target_property),
                            function,
                        },
                    },
                    request_id,
                    TimestampMillis::now(),
                )
                .await?;
            converted.rollup_values += u32::from(made);
        }
        Ok(converted)
    }
}

/// Notion's rollup function names (API `rollup.function`); the per-group ones have no match.
fn rollup_function(name: &str) -> Option<RollupFunction> {
    use RollupFunction as F;
    Some(match name {
        "show_original" => F::ShowOriginal,
        "show_unique" => F::ShowUnique,
        "count" | "count_all" => F::CountAll,
        "count_values" => F::CountValues,
        "unique" | "count_unique_values" => F::CountUnique,
        "empty" | "count_empty" => F::CountEmpty,
        "not_empty" | "count_not_empty" => F::CountNotEmpty,
        "percent_empty" => F::PercentEmpty,
        "percent_not_empty" => F::PercentNotEmpty,
        "sum" => F::Sum,
        "average" => F::Average,
        "median" => F::Median,
        "min" => F::Min,
        "max" => F::Max,
        "range" => F::Range,
        "earliest_date" => F::EarliestDate,
        "latest_date" => F::LatestDate,
        "date_range" => F::DateRange,
        "checked" => F::Checked,
        "unchecked" => F::Unchecked,
        "percent_checked" => F::PercentChecked,
        "percent_unchecked" => F::PercentUnchecked,
        _ => return None,
    })
}
