//! Port of `Engine/RenderMetrics.swift`: geometry constants shared by the
//! engine, the fragments, and the view.
//!
//! These are *structure*, not taste: the palette and the type scale live in
//! `StyleSheet` (§11.1, §11.2).

use objc2_foundation::NSPoint;

/// Swift's `FloatingPointRoundingRule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingRule {
    ToNearestOrAwayFromZero,
    ToNearestOrEven,
    Up,
    Down,
    TowardZero,
    AwayFromZero,
}

impl RoundingRule {
    /// `value.rounded(rule)`.
    #[inline]
    pub fn apply(self, value: f64) -> f64 {
        match self {
            RoundingRule::ToNearestOrAwayFromZero => value.round(),
            RoundingRule::ToNearestOrEven => value.round_ties_even(),
            RoundingRule::Up => value.ceil(),
            RoundingRule::Down => value.floor(),
            RoundingRule::TowardZero => value.trunc(),
            RoundingRule::AwayFromZero => {
                if value < 0.0 {
                    value.floor()
                } else if value > 0.0 {
                    value.ceil()
                } else {
                    value
                }
            }
        }
    }
}

/// Width of the left rail that block markers live in (§6.1a).
pub const GUTTER_WIDTH: f64 = 44.0;

/// Slack reserved to the left of the text column so a caret-anchored reveal
/// (§6.1c) can shift a line *left* without clipping.
pub const REVEAL_SLACK: f64 = 44.0;

/// Vertical inset at the top and bottom of the text container.
pub const VERTICAL_INSET: f64 = 36.0;

/// Lane past the prose measure's trailing edge that a *full-bleed* block is
/// allowed to spill into (§11.1). Trailing only, and a constant.
pub const CODE_BLEED: f64 = 88.0;

/// Narrowest prose column worth laying out.
pub const MINIMUM_PROSE_WIDTH: f64 = 240.0;

/// Padding inside a code block's tinted band.
pub const CODE_INSET_X: f64 = 22.0;
/// Bottom chrome (closing fence) height.
pub const CODE_INSET_Y: f64 = 14.0;
/// Real header row for the language chip and copy control (§11.3).
pub const CODE_HEADER_HEIGHT: f64 = 36.0;
pub const CODE_RULE_WIDTH: f64 = 2.0;
pub const CODE_CORNER_RADIUS: f64 = 10.0;
/// Columns a tab advances inside a code block.
pub const CODE_TAB_COLUMNS: i64 = 4;
/// Corner of an inline code pill.
pub const INLINE_CODE_CORNER_RADIUS: f64 = 3.0;

/// Page background above and below a fenced block.
pub const CODE_BLOCK_GAP: f64 = 8.0;

/// §11.3 task-checkbox geometry, shared by the document renderer and the task
/// panel.
pub const TASK_BOX_SIDE: f64 = 20.0;
pub const TASK_BOX_GAP: f64 = 11.0;
pub const TASK_BOX_CLEARANCE: f64 = 4.0;

/// `taskBoxSide + taskBoxGap + taskBoxClearance`, evaluated left to right.
#[inline]
pub fn task_marker_column() -> f64 {
    TASK_BOX_SIDE + TASK_BOX_GAP + TASK_BOX_CLEARANCE
}

/// The box and its tick as ratios of the side.
pub const TASK_BOX_CORNER_RATIO: f64 = 5.5 / 20.0;
pub const TASK_BOX_STROKE_RATIO: f64 = 1.5 / 20.0;
pub const TASK_TICK_STROKE_RATIO: f64 = 2.0 / 20.0;

/// The tick, in unit coordinates measured from the bottom-left of the box.
pub const TASK_TICK: [NSPoint; 3] = [
    NSPoint::new(0.245, 0.500),
    NSPoint::new(0.430, 0.315),
    NSPoint::new(0.765, 0.690),
];

/// One-line chip a long code block collapses to in Read mode (§5.1).
pub const CHIP_HEIGHT: f64 = 30.0;
/// Code blocks longer than this collapse in Read mode (§5.1).
pub const CODE_COLLAPSE_LINE_COUNT: i64 = 20;

pub const CALLOUT_RULE_WIDTH: f64 = 3.0;
pub const CALLOUT_INSET_X: f64 = 16.0;
pub const CALLOUT_ICON_INSET_X: f64 = 34.0;
pub const CALLOUT_INSET_Y: f64 = 8.0;
pub const CALLOUT_CORNER_RADIUS: f64 = 8.0;
pub const QUOTE_RULE_WIDTH: f64 = 2.0;

pub const TABLE_ROW_PADDING: f64 = 6.0;
pub const TABLE_COLUMN_GAP: f64 = 18.0;
pub const TABLE_RULE_WIDTH: f64 = 1.0;

pub const IMAGE_CORNER_RADIUS: f64 = 8.0;
pub const IMAGE_SHADOW_RADIUS: f64 = 10.0;
pub const IMAGE_CAPTION_GAP: f64 = 8.0;

pub const THEMATIC_BREAK_SPACE: f64 = 26.0;

/// Indentation applied per level of list or quote nesting.
#[inline]
pub fn indent_unit(body_size: f64) -> f64 {
    (body_size * 1.45).round()
}

/// Snaps a height to the baseline grid so structural-zoom transitions animate
/// cleanly (§11.1). Rounds *up* by default (`snap(value, grid)`); code passes
/// `RoundingRule::Down`.
#[inline]
#[allow(clippy::neg_cmp_op_on_partial_ord)] // `guard grid > 0.5 else`: NaN takes the guard
pub fn snap(value: f64, grid: f64, rounding: RoundingRule) -> f64 {
    if !(grid > 0.5) {
        return value.round();
    }
    rounding.apply(value / grid) * grid
}

/// `snap(value, grid:)` with the default `.up` rule.
#[inline]
pub fn snap_up(value: f64, grid: f64) -> f64 {
    snap(value, grid, RoundingRule::Up)
}
