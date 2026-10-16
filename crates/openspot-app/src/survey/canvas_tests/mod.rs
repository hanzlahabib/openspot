//! Tests for the survey-canvas features, one module per feature.

pub(super) use super::*;

fn ap(last: u8, rssi: i32) -> AccessPoint {
    AccessPoint::from_beacon("n".into(), MacAddress([0, 0, 0, 0, 0, last]), rssi, 2_437_000, true, 100, vec![])
}

mod background_pipeline;
mod path_run;
mod sampling_flow;
mod tx_rate;
mod structure_undo;
mod crop;
mod draw;
mod export;
mod hidden_networks;
mod manual_ap;
mod merge;
mod pdf;
mod visibility;
