//! Calendar views (spec P3 13.2): the date property that places the rows, and month or week.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Property, PropertyKind, ViewConfig, ViewType};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseCalendarMode)]
pub enum CalendarMode {
    #[default]
    Month,
    Week,
}

impl CalendarMode {
    pub(super) fn is_month(&self) -> bool {
        *self == Self::Month
    }
}

/// A property whose value can place a row on a calendar.
const fn datable(kind: &PropertyKind) -> bool {
    matches!(
        kind,
        PropertyKind::Date(_) | PropertyKind::CreatedTime | PropertyKind::LastEditedTime
    )
}

/// The calendar part of `ViewConfig::repair`: a date property that is gone or cannot place rows
/// is dropped; a calendar without one takes the first `date` property (none when there is none).
pub(super) fn repair(config: &mut ViewConfig, view_type: ViewType, properties: &[Property]) {
    let kept = config.date_property_id.filter(|id| {
        properties
            .iter()
            .any(|property| property.id == *id && datable(&property.kind))
    });
    config.date_property_id = match view_type {
        ViewType::Calendar => kept.or_else(|| {
            properties
                .iter()
                .find(|property| matches!(property.kind, PropertyKind::Date(_)))
                .map(|property| property.id)
        }),
        _ => kept,
    };
}

/// Checks the calendar fields a client sends; errors name the field path.
pub(super) fn check(config: &ViewConfig, properties: &[Property]) -> Result<(), String> {
    let unfit = properties
        .iter()
        .find(|property| Some(property.id) == config.date_property_id)
        .is_some_and(|property| !datable(&property.kind));
    if unfit {
        return Err("config.date_property_id".to_owned());
    }
    Ok(())
}
