//! Port of `Mermaid/src_sequence_layout.swift` (from
//! `original/src/sequence/layout.ts`).

use std::collections::HashMap;

use super::src_sequence_parser::*;
use super::src_styles::{estimate_text_width, FONT_SIZES, FONT_WEIGHTS};
use super::src_types::SDict;
use crate::error::MermaidError;
use crate::swift::{self, max, min};

const PADDING: f64 = 30.0;
const ACTOR_GAP: f64 = 140.0;
const ACTOR_HEIGHT: f64 = 40.0;
const ACTOR_PAD_X: f64 = 16.0;
const HEADER_GAP: f64 = 20.0;
const MESSAGE_ROW_HEIGHT: f64 = 40.0;
const SELF_MESSAGE_HEIGHT: f64 = 30.0;
const ACTIVATION_WIDTH: f64 = 10.0;
const BLOCK_PAD_X: f64 = 10.0;
const BLOCK_PAD_TOP: f64 = 40.0;
const BLOCK_PAD_BOTTOM: f64 = 8.0;
const BLOCK_HEADER_EXTRA: f64 = 28.0;
const DIVIDER_EXTRA: f64 = 24.0;
const NOTE_WIDTH: f64 = 120.0;
const NOTE_PADDING: f64 = 8.0;
const NOTE_GAP: f64 = 10.0;

fn note_lines(text: &str) -> Vec<&str> {
    let lines = swift::components_separated_by_newline(text);
    if lines.is_empty() { vec![""] } else { lines }
}

fn note_height(note: &SequenceNote) -> f64 {
    let non_empty = note_lines(&note.text);
    let line_count = max(1, non_empty.len()) as f64;
    let line_height = FONT_SIZES.edge_label.ceil();
    let line_spacing = 4.0;
    line_count * line_height + max(0.0, line_count - 1.0) * line_spacing + NOTE_PADDING * 2.0
}

/// `layoutSequenceDiagram(_:_:)`.
pub fn layout_sequence_diagram(diagram: &SequenceDiagram) -> Result<PositionedSequenceDiagram, MermaidError> {
    if diagram.actors.is_empty() {
        return Ok(PositionedSequenceDiagram {
            width: 0.0,
            height: 0.0,
            actors: vec![],
            lifelines: vec![],
            messages: vec![],
            activations: vec![],
            blocks: vec![],
            notes: vec![],
        });
    }

    let actor_widths: Vec<f64> = diagram
        .actors
        .iter()
        .map(|actor| {
            let text_w = estimate_text_width(&actor.label, FONT_SIZES.node_label, FONT_WEIGHTS.node_label);
            max(text_w + ACTOR_PAD_X * 2.0, 80.0)
        })
        .collect();

    let mut actor_center_x: Vec<f64> = Vec::with_capacity(diagram.actors.len());
    let mut current_x = PADDING + actor_widths[0] / 2.0;
    for i in 0..diagram.actors.len() {
        if i > 0 {
            let min_gap = max(ACTOR_GAP, (actor_widths[i - 1] + actor_widths[i]) / 2.0 + 40.0);
            current_x += min_gap;
        }
        actor_center_x.push(current_x);
    }

    let mut actor_index: SDict<usize> = SDict::new();
    for (i, actor) in diagram.actors.iter().enumerate() {
        actor_index.insert(&actor.id, i);
    }
    let index_of = |id: &str| actor_index.get(id).copied().unwrap_or(0);

    let actor_y = PADDING;
    let actors: Vec<PositionedSequenceActor> = diagram
        .actors
        .iter()
        .enumerate()
        .map(|(idx, actor)| PositionedSequenceActor {
            id: actor.id.clone(),
            label: actor.label.clone(),
            r#type: actor.r#type.clone(),
            x: actor_center_x[idx],
            y: actor_y,
            width: actor_widths[idx],
            height: ACTOR_HEIGHT,
        })
        .collect();

    let mut message_y = actor_y + ACTOR_HEIGHT + HEADER_GAP;
    let mut messages: Vec<PositionedSequenceMessage> = Vec::new();

    let mut extra_space_before: HashMap<i64, f64> = HashMap::new();
    for block in &diagram.blocks {
        let e = extra_space_before.get(&block.start_index).copied().unwrap_or(0.0);
        extra_space_before.insert(block.start_index, max(e, BLOCK_HEADER_EXTRA));
        for div in &block.dividers {
            let e = extra_space_before.get(&div.index).copied().unwrap_or(0.0);
            extra_space_before.insert(div.index, max(e, DIVIDER_EXTRA));
        }
    }

    // Swift `[String: [(startY, depth)]]`, iterated at the end in Dictionary
    // order; keep first-insertion order so the result is deterministic (the
    // activations of different actors never overlap).
    let mut activation_stacks: SDict<Vec<(f64, i64)>> = SDict::new();
    let mut stack_order: Vec<String> = Vec::new();
    let mut activations: Vec<SequenceActivation> = Vec::new();
    let nesting_offset = 4.0;

    for msg_idx in 0..diagram.messages.len() {
        let msg = &diagram.messages[msg_idx];
        let from_idx = index_of(&msg.from);
        let to_idx = index_of(&msg.to);
        let is_self_msg = swift::string_eq(&msg.from, &msg.to);

        let extra = extra_space_before.get(&(msg_idx as i64)).copied().unwrap_or(0.0);
        if extra > 0.0 {
            message_y += extra;
        }

        // Push messageY down if a note sits between the previous message and this one
        for note in diagram.notes.iter().filter(|n| n.after_index == msg_idx as i64 - 1) {
            let note_h = note_height(note);
            let note_bottom = message_y + 4.0 + note_h;
            let required_y = note_bottom + NOTE_GAP;
            message_y = max(message_y, required_y);
        }

        let x1 = actor_center_x[from_idx];
        let x2 = actor_center_x[to_idx];

        messages.push(PositionedSequenceMessage {
            from: msg.from.clone(),
            to: msg.to.clone(),
            label: msg.label.clone(),
            line_style: msg.line_style.clone(),
            arrow_head: msg.arrow_head.clone(),
            x1,
            x2,
            y: message_y,
            is_self: is_self_msg,
        });

        if msg.activate {
            if !activation_stacks.contains_key(&msg.to) {
                stack_order.push(msg.to.clone());
            }
            let stack = activation_stacks.entry_or(&msg.to, Vec::new);
            let depth = stack.len() as i64;
            stack.push((message_y, depth));
        }

        if msg.deactivate {
            if !activation_stacks.contains_key(&msg.from) {
                stack_order.push(msg.from.clone());
            }
            let stack = activation_stacks.entry_or(&msg.from, Vec::new);
            if let Some(top) = stack.pop() {
                let idx = index_of(&msg.from);
                let x_offset = top.1 as f64 * nesting_offset;
                activations.push(SequenceActivation {
                    actor_id: msg.from.clone(),
                    x: actor_center_x[idx] - ACTIVATION_WIDTH / 2.0 + x_offset,
                    top_y: top.0,
                    bottom_y: message_y,
                    width: ACTIVATION_WIDTH,
                });
            }
        }

        message_y += if is_self_msg { SELF_MESSAGE_HEIGHT + MESSAGE_ROW_HEIGHT } else { MESSAGE_ROW_HEIGHT };
    }

    for actor_id in &stack_order {
        let stack = activation_stacks.get(actor_id).unwrap();
        for item in stack {
            let idx = index_of(actor_id);
            let x_offset = item.1 as f64 * nesting_offset;
            activations.push(SequenceActivation {
                actor_id: actor_id.clone(),
                x: actor_center_x[idx] - ACTIVATION_WIDTH / 2.0 + x_offset,
                top_y: item.0,
                bottom_y: message_y - MESSAGE_ROW_HEIGHT / 2.0,
                width: ACTIVATION_WIDTH,
            });
        }
    }

    let message_count = diagram.messages.len() as i64;
    let blocks: Vec<PositionedSequenceBlock> = diagram
        .blocks
        .iter()
        .map(|block| {
            let start_msg = if block.start_index < messages.len() as i64 { messages.get(block.start_index as usize) } else { None };
            let end_msg = if block.end_index < messages.len() as i64 { messages.get(block.end_index as usize) } else { None };
            let block_top = start_msg.map_or(message_y, |m| m.y) - BLOCK_PAD_TOP;
            let block_bottom = end_msg.map_or(message_y, |m| m.y) + BLOCK_PAD_BOTTOM + 12.0;

            // `Set<Int>`: only its min and max are read.
            let mut involved: Vec<usize> = Vec::new();
            if block.start_index <= block.end_index {
                for mi in block.start_index..=block.end_index {
                    if mi >= 0 && mi < message_count {
                        let m = &diagram.messages[mi as usize];
                        involved.push(index_of(&m.from));
                        involved.push(index_of(&m.to));
                    }
                }
            }

            if involved.is_empty() {
                involved.extend(0..diagram.actors.len());
            }

            let min_idx = involved.iter().copied().min().unwrap_or(0);
            let max_idx = involved.iter().copied().max().unwrap_or(max(0, diagram.actors.len() as i64 - 1) as usize);
            let block_left = actor_center_x[min_idx] - actor_widths[min_idx] / 2.0 - BLOCK_PAD_X;
            let block_right = actor_center_x[max_idx] + actor_widths[max_idx] / 2.0 + BLOCK_PAD_X;

            let positioned_dividers = block
                .dividers
                .iter()
                .map(|divider| {
                    let msg = if divider.index < messages.len() as i64 { messages.get(divider.index as usize) } else { None };
                    let msg_y = msg.map_or(message_y, |m| m.y);
                    let mut offset = 28.0;

                    if let (false, Some(msg)) = (divider.label.is_empty(), msg) {
                        let div_label_text = format!("[{}]", divider.label);
                        let div_label_w = estimate_text_width(&div_label_text, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label);
                        let div_label_left = block_left + 8.0;
                        let div_label_right = div_label_left + div_label_w;

                        let msg_label_w = estimate_text_width(&msg.label, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label);
                        let msg_label_left =
                            if msg.is_self { msg.x1 + 36.0 } else { (msg.x1 + msg.x2) / 2.0 - msg_label_w / 2.0 };
                        let msg_label_right = msg_label_left + msg_label_w;

                        if div_label_right > msg_label_left && div_label_left < msg_label_right {
                            offset = 36.0;
                        }
                    }

                    PositionedSequenceBlockDivider { y: msg_y - offset, label: divider.label.clone() }
                })
                .collect();

            PositionedSequenceBlock {
                r#type: block.r#type.clone(),
                label: block.label.clone(),
                x: block_left,
                y: block_top,
                width: block_right - block_left,
                height: block_bottom - block_top,
                dividers: positioned_dividers,
            }
        })
        .collect();

    let notes: Vec<PositionedSequenceNote> = diagram
        .notes
        .iter()
        .map(|note| {
            let non_empty = note_lines(&note.text);
            let max_line_width = swift::seq_max(
                non_empty.iter().map(|l| estimate_text_width(l, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label)),
            )
            .unwrap_or(0.0);
            let note_w = max(NOTE_WIDTH, max_line_width + NOTE_PADDING * 2.0);
            let note_h = note_height(note);

            let ref_msg = if note.after_index >= 0 && note.after_index < messages.len() as i64 {
                messages.get(note.after_index as usize)
            } else {
                None
            };
            let note_y = ref_msg.map_or(actor_y + ACTOR_HEIGHT, |m| m.y) + 4.0;

            let first_actor_idx = actor_index.get(note.actor_ids.first().map_or("", String::as_str)).copied().unwrap_or(0);
            let note_x = if note.position == "left" {
                actor_center_x[first_actor_idx] - actor_widths[first_actor_idx] / 2.0 - note_w - NOTE_GAP
            } else if note.position == "right" {
                actor_center_x[first_actor_idx] + actor_widths[first_actor_idx] / 2.0 + NOTE_GAP
            } else if note.actor_ids.len() > 1 {
                let last_actor_idx =
                    actor_index.get(note.actor_ids.last().map_or("", String::as_str)).copied().unwrap_or(first_actor_idx);
                (actor_center_x[first_actor_idx] + actor_center_x[last_actor_idx]) / 2.0 - note_w / 2.0
            } else {
                actor_center_x[first_actor_idx] - note_w / 2.0
            };

            PositionedSequenceNote {
                text: note.text.clone(),
                x: note_x,
                y: note_y,
                width: note_w,
                height: note_h,
                position: note.position.clone(),
                actors: note.actor_ids.clone(),
            }
        })
        .collect();

    let diagram_bottom = message_y + PADDING;

    let mut global_min_x = PADDING;
    let mut global_max_x = 0.0;

    for actor in &actors {
        global_min_x = min(global_min_x, actor.x - actor.width / 2.0);
        global_max_x = max(global_max_x, actor.x + actor.width / 2.0);
    }
    for block in &blocks {
        global_min_x = min(global_min_x, block.x);
        global_max_x = max(global_max_x, block.x + block.width);
    }
    for note in &notes {
        global_min_x = min(global_min_x, note.x);
        global_max_x = max(global_max_x, note.x + note.width);
    }
    for msg in messages.iter().filter(|m| m.is_self) {
        let loop_w = 30.0;
        let label_padding = 8.0;
        let label_left = msg.x1 + loop_w + label_padding;
        let label_width = estimate_text_width(&msg.label, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label);
        global_max_x = max(global_max_x, label_left + label_width + 8.0);
    }

    let shift_x = if global_min_x < PADDING { PADDING - global_min_x } else { 0.0 };

    let mut shifted_actors = actors;
    let mut shifted_messages = messages;
    let mut shifted_activations = activations;
    let mut shifted_blocks = blocks;
    let mut shifted_notes = notes;

    if shift_x > 0.0 {
        for a in &mut shifted_actors {
            a.x += shift_x;
        }
        for m in &mut shifted_messages {
            m.x1 += shift_x;
            m.x2 += shift_x;
        }
        for a in &mut shifted_activations {
            a.x += shift_x;
        }
        for b in &mut shifted_blocks {
            b.x += shift_x;
        }
        for n in &mut shifted_notes {
            n.x += shift_x;
        }
        for x in &mut actor_center_x {
            *x += shift_x;
        }
    }

    let lifelines = diagram
        .actors
        .iter()
        .enumerate()
        .map(|(idx, actor)| SequenceLifeline {
            actor_id: actor.id.clone(),
            x: actor_center_x[idx],
            top_y: actor_y + ACTOR_HEIGHT,
            bottom_y: diagram_bottom - PADDING,
        })
        .collect();

    let diagram_width = global_max_x + shift_x + PADDING;
    let diagram_height = diagram_bottom;

    Ok(PositionedSequenceDiagram {
        width: max(diagram_width, 200.0),
        height: max(diagram_height, 100.0),
        actors: shifted_actors,
        lifelines,
        messages: shifted_messages,
        activations: shifted_activations,
        blocks: shifted_blocks,
        notes: shifted_notes,
    })
}
