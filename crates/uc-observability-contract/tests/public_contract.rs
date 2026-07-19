use std::str::FromStr;

use uc_observability_contract::analytics::{AnalyticsPort, Event, NoopAnalyticsSink};
use uc_observability_contract::FlowId;

#[test]
fn flow_id_round_trips_through_its_wire_string() {
    let generated = FlowId::generate();
    let parsed = FlowId::from_str(&generated.to_string()).expect("flow id should parse");

    assert_eq!(parsed, generated);
}

#[test]
fn event_names_remain_stable_without_a_runtime_sink() {
    let event = Event::AppOpened;
    assert_eq!(event.name(), "app_opened");

    let sink: &dyn AnalyticsPort = &NoopAnalyticsSink;
    sink.capture(event);
}
