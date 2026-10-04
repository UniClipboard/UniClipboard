//! The pages shown in place of a list: first use, locked, disconnected and no match, and the
//! suggestions to relax a search that found nothing.

use crate::query::filters::{Dimension, Filters};
use crate::text::{fill, t, value_label};

/// A search with one condition loosened, offered when nothing matched.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Relaxation {
    pub label: String,
    pub filters: Filters,
}

/// The ways to loosen `filters`, one per dimension that has values, in the order type, tag, time
/// and device. `device_name` turns a device id into what the user knows it by.
pub fn relaxations(filters: &Filters, device_name: impl Fn(&str) -> String) -> Vec<Relaxation> {
    let mut result = vec![];
    let mut without = |dimension: Dimension, label: String| {
        let mut relaxed = filters.clone();
        relaxed.clear(dimension);
        result.push(Relaxation {
            label,
            filters: relaxed,
        });
    };
    match filters.types.as_slice() {
        [] => {}
        [only] => without(
            Dimension::Type,
            fill(t().remove_type, &[("value", value_label(only))]),
        ),
        _ => without(Dimension::Type, t().remove_all_types.into()),
    }
    match filters.tags.as_slice() {
        [] => {}
        [only] => without(
            Dimension::Tag,
            fill(t().remove_tag, &[("value", value_label(only))]),
        ),
        _ => without(Dimension::Tag, t().remove_all_tags.into()),
    }
    if filters.time.is_some() {
        without(Dimension::Time, t().widen_time.into());
    }
    match filters.sources.as_slice() {
        [] => {}
        [only] => without(
            Dimension::Source,
            fill(t().remove_device, &[("name", &device_name(only))]),
        ),
        _ => without(Dimension::Source, t().include_all_devices.into()),
    }
    result
}

/// What the list area shows when there is nothing to list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Empty {
    /// No history at all and nothing typed.
    FirstUse,
    Locked,
    Disconnected,
    /// Something was typed or filtered and nothing matched.
    NoMatch,
}

pub fn classify(locked: bool, disconnected: bool, searching: bool) -> Empty {
    if locked {
        Empty::Locked
    } else if disconnected {
        Empty::Disconnected
    } else if searching {
        Empty::NoMatch
    } else {
        Empty::FirstUse
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::date_range;

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()
    }

    #[test]
    fn each_dimension_with_values_can_be_dropped_on_its_own() {
        let mut filters = Filters {
            query: "kubectl".into(),
            ..Default::default()
        };
        filters.add(Dimension::Type, "text".into());
        filters.add(Dimension::Tag, "code".into());
        filters.set_time(date_range::parse("上周", today()).unwrap());
        filters.add(Dimension::Source, "p1".into());
        let options = relaxations(&filters, |id| format!("dev-{id}"));
        let labels: Vec<_> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "去掉 /文本",
                "去掉 #代码",
                "时间范围扩大到全部",
                "去掉 @dev-p1，包含所有设备"
            ]
        );
        // Only the named dimension is loosened; the query stays.
        assert!(options[0].filters.types.is_empty());
        assert_eq!(options[0].filters.tags, ["code"]);
        assert_eq!(options[0].filters.query, "kubectl");
        assert!(options[2].filters.time.is_none() && !options[2].filters.sources.is_empty());
        assert!(options[3].filters.sources.is_empty() && options[3].filters.time.is_some());
    }

    #[test]
    fn several_values_are_dropped_together() {
        let mut filters = Filters::default();
        filters.add(Dimension::Tag, "a".into());
        filters.add(Dimension::Tag, "b".into());
        filters.add(Dimension::Source, "p1".into());
        filters.add(Dimension::Source, "p2".into());
        let labels: Vec<_> = relaxations(&filters, str::to_string)
            .into_iter()
            .map(|o| o.label)
            .collect();
        assert_eq!(labels, ["去掉所有标签", "包含所有设备"]);
    }

    #[test]
    fn no_conditions_means_nothing_to_relax() {
        assert!(relaxations(&Filters::default(), str::to_string).is_empty());
    }

    #[test]
    fn the_empty_pages_are_told_apart() {
        assert_eq!(classify(true, true, true), Empty::Locked);
        assert_eq!(classify(false, true, false), Empty::Disconnected);
        assert_eq!(classify(false, false, true), Empty::NoMatch);
        assert_eq!(classify(false, false, false), Empty::FirstUse);
    }
}
