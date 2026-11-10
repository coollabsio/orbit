//! Timeline views (spec P3 16.2): the date properties that place the bars, the zoom and the table
//! pane.

use std::collections::HashSet;

use orbit_platform::Id;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{MAX_PROPERTIES, Property, PropertyKind, ViewConfig, ViewType};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseTimelineZoom)]
pub enum TimelineZoom {
    Day,
    Week,
    #[default]
    Month,
    Quarter,
    Year,
}

impl TimelineZoom {
    pub(super) fn is_month(&self) -> bool {
        *self == Self::Month
    }
}

fn is_date(properties: &[Property], id: Id) -> bool {
    properties
        .iter()
        .any(|property| property.id == id && matches!(property.kind, PropertyKind::Date(_)))
}

/// The timeline part of `ViewConfig::repair`: a start or end property that is gone or is not a
/// `date` is dropped (a timeline then starts at the first `date` property), an end equal to the
/// start is dropped, and table columns keep known non-title properties once.
pub(super) fn repair(config: &mut ViewConfig, view_type: ViewType, properties: &[Property]) {
    let start = config
        .start_property_id
        .filter(|id| is_date(properties, *id));
    config.start_property_id = match view_type {
        ViewType::Timeline => start.or_else(|| {
            properties
                .iter()
                .find(|property| matches!(property.kind, PropertyKind::Date(_)))
                .map(|property| property.id)
        }),
        _ => start,
    };
    config.end_property_id = config
        .end_property_id
        .filter(|id| is_date(properties, *id) && Some(*id) != config.start_property_id);
    let mut seen = HashSet::new();
    config.table_columns.retain(|id| {
        properties
            .iter()
            .any(|property| property.id == *id && property.kind != PropertyKind::Title)
            && seen.insert(*id)
    });
}

/// Checks the timeline fields a client sends; errors name the field path.
pub(super) fn check(config: &ViewConfig, properties: &[Property]) -> Result<(), String> {
    let unfit = |id: Option<Id>| {
        properties
            .iter()
            .find(|property| Some(property.id) == id)
            .is_some_and(|property| !matches!(property.kind, PropertyKind::Date(_)))
    };
    if unfit(config.start_property_id) {
        return Err("config.start_property_id".to_owned());
    }
    if unfit(config.end_property_id) {
        return Err("config.end_property_id".to_owned());
    }
    if config.table_columns.len() > MAX_PROPERTIES {
        return Err("config.table_columns".to_owned());
    }
    Ok(())
}
