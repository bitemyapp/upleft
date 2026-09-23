//! Port of `core/data/LayoutMetaDataService.swift`.
//!
//! `ELK.init` registers the layered algorithm and nothing else: no layout
//! options are ever registered, so `getOptionData` always returns `nil` and
//! the JSON importer always falls back to its own value parsing.

use std::rc::Rc;

use super::layout_algorithm_data::LayoutAlgorithmData;
use crate::org::eclipse::elk::graph::properties::graph_feature::GraphFeature;

thread_local! {
    static LAYERED: Rc<LayoutAlgorithmData> = Rc::new(LayoutAlgorithmData {
        id: "org.eclipse.elk.layered",
        name: "ELK Layered",
        supported_features: &[
            GraphFeature::SELF_LOOPS,
            GraphFeature::INSIDE_SELF_LOOPS,
            GraphFeature::MULTI_EDGES,
            GraphFeature::EDGE_LABELS,
            GraphFeature::PORTS,
            GraphFeature::COMPOUND,
            GraphFeature::CLUSTERS,
        ],
    });
}

pub struct LayoutMetaDataService;

impl LayoutMetaDataService {
    /// `getAlgorithmData(by: suffix)`: the unique algorithm whose id equals the
    /// suffix or ends with `.` + suffix.
    pub fn get_algorithm_data_by_suffix(suffix: &str) -> Option<Rc<LayoutAlgorithmData>> {
        if suffix.is_empty() {
            return None;
        }
        LAYERED.with(|d| {
            let id = d.id;
            // Swift compares `Character` counts; the ids are ASCII.
            let matches = id.ends_with(suffix)
                && (suffix.chars().count() == id.chars().count()
                    || id.chars().rev().nth(suffix.chars().count()) == Some('.'));
            if matches { Some(d.clone()) } else { None }
        })
    }
}
