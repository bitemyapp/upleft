//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/org_eclipse_elk_alg_layered_intermediate_loops_SelfHyperLoopLabels.swift`.
//!
//! The labels of one self hyper loop, placed as one block. `size` and
//! `position` are private vectors of this object (never aliased).

use super::self_loop_port::SlPortId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LLabelId, LNodeId};
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

/// `SelfHyperLoopLabels.Alignment`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Alignment {
    CENTER,
    LEFT,
    RIGHT,
    TOP,
}

#[derive(Clone, Debug)]
pub struct SelfHyperLoopLabels {
    pub id: i64,

    l_labels: Vec<LLabelId>,
    size: KVector,
    position: KVector,
    layout_direction: Direction,
    label_label_spacing: f64,

    side: PortSide,
    alignment: Alignment,
    alignment_reference_sl_port: Option<SlPortId>,
}

impl SelfHyperLoopLabels {
    /// `init(_ slLoop:)`; `l_node` is the loop's holder's node.
    pub fn new(lg: &LGraphArena, l_node: LNodeId) -> SelfHyperLoopLabels {
        let layout_direction = lg
            .node_graph(l_node)
            .and_then(|g| lg[g].props.get_as::<Direction>(&LayeredOptions::DIRECTION))
            .unwrap_or(Direction::RIGHT);
        let label_label_spacing = lg.get_individual_or_inherited(l_node, &LayeredOptions::SPACING_LABEL_LABEL);
        SelfHyperLoopLabels {
            id: 0,
            l_labels: Vec::new(),
            size: KVector::default(),
            position: KVector::default(),
            layout_direction,
            label_label_spacing,
            side: PortSide::UNDEFINED,
            alignment: Alignment::CENTER,
            alignment_reference_sl_port: None,
        }
    }

    // MARK: - LLabel Access

    pub fn add_l_labels(&mut self, lg: &LGraphArena, new_l_labels: &[LLabelId]) {
        for &new_l_label in new_l_labels {
            self.l_labels.push(new_l_label);
            self.update_size(lg[new_l_label].size);
        }
    }

    pub fn get_l_labels(&self) -> &[LLabelId] {
        &self.l_labels
    }

    fn update_size(&mut self, new_l_label_size: KVector) {
        if self.layout_direction.is_horizontal() {
            self.size.x = swift::max(self.size.x, new_l_label_size.x);
            self.size.y += new_l_label_size.y;
            if self.l_labels.len() > 1 {
                self.size.y += self.label_label_spacing;
            }
        } else {
            self.size.x += new_l_label_size.x;
            self.size.y = swift::max(self.size.y, new_l_label_size.y);
            if self.l_labels.len() > 1 {
                self.size.x += self.label_label_spacing;
            }
        }
    }

    // `applyLabelManagement(_:_:)` is only called with a label manager, which
    // the JSON bridge can never provide (there is no `ILabelManager` value a
    // layout option could hold), so it is unreachable and not ported.

    pub fn apply_placement(&self, lg: &mut LGraphArena, offset: KVector) {
        if self.layout_direction.is_horizontal() {
            self.apply_placement_for_horizontal_layout(lg, offset);
        } else {
            self.apply_placement_for_vertical_layout(lg, offset);
        }
    }

    fn apply_placement_for_horizontal_layout(&self, lg: &mut LGraphArena, offset: KVector) {
        let x = self.position.x;
        let mut y = self.position.y;

        for &l_label in &self.l_labels {
            let label = &mut lg[l_label];
            let label_size = label.size;
            let label_pos = &mut label.position;

            if self.alignment == Alignment::LEFT || self.side == PortSide::EAST {
                label_pos.x = x;
            } else if self.alignment == Alignment::RIGHT || self.side == PortSide::WEST {
                label_pos.x = x + self.size.x - label_size.x;
            } else {
                label_pos.x = x + (self.size.x - label_size.x) / 2.0;
            }

            label_pos.y = y;
            label_pos.add(offset);

            y += label_size.y + self.label_label_spacing;
        }
    }

    fn apply_placement_for_vertical_layout(&self, lg: &mut LGraphArena, offset: KVector) {
        let mut x = self.position.x;
        let y = self.position.y;

        for &l_label in &self.l_labels {
            let label = &mut lg[l_label];
            let label_size = label.size;
            let label_pos = &mut label.position;

            label_pos.x = x;

            if self.side == PortSide::NORTH {
                label_pos.y = y + self.size.y - label_size.y;
            } else {
                label_pos.y = y;
            }

            label_pos.add(offset);

            x += label_size.x + self.label_label_spacing;
        }
    }

    // MARK: - Label Placement

    pub fn get_size(&self) -> KVector {
        self.size
    }

    pub fn get_position(&self) -> KVector {
        self.position
    }

    /// `getPosition()` for callers that mutate the returned vector.
    pub fn position_mut(&mut self) -> &mut KVector {
        &mut self.position
    }

    pub fn get_side(&self) -> PortSide {
        self.side
    }

    pub fn set_side(&mut self, side: PortSide) {
        self.side = side;
    }

    pub fn get_alignment(&self) -> Alignment {
        self.alignment
    }

    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    pub fn get_alignment_reference_sl_port(&self) -> Option<SlPortId> {
        self.alignment_reference_sl_port
    }

    pub fn set_alignment_reference_sl_port(&mut self, port: Option<SlPortId>) {
        self.alignment_reference_sl_port = port;
    }
}
