//! Port of `Mermaid/src_xychart_layout.swift` (from `original/src/xychart/layout.ts`).

use super::src_styles::estimate_text_width;
use super::src_xychart_types::*;
use crate::swift::{self, max, min};

const PLOT_WIDTH: f64 = 600.0;
const PLOT_HEIGHT: f64 = 340.0;
const PADDING: f64 = 22.0;
const TITLE_FONT_SIZE: f64 = 18.0;
const TITLE_HEIGHT: f64 = 42.0;
const AXIS_LABEL_FONT_SIZE: f64 = 14.0;
const AXIS_LABEL_FONT_WEIGHT: i64 = 400;
const X_LABEL_HEIGHT: f64 = 38.0;
const Y_LABEL_WIDTH: f64 = 58.0;
const Y_LABEL_GAP: f64 = 18.0;
const AXIS_TITLE_PAD: f64 = 30.0;
const TICK_LENGTH: f64 = 4.0;
const BAR_PAD_RATIO: f64 = 0.2;
const BAR_GROUP_GAP: f64 = 0.0;
const MAX_BAR_WIDTH: f64 = 40.0;
const LEGEND_FONT_SIZE: f64 = 12.0;
const LEGEND_FONT_WEIGHT: i64 = 400;
const LEGEND_HEIGHT: f64 = 24.0;
const LEGEND_SWATCH_W: f64 = 12.0;
const LEGEND_GAP: f64 = 5.0;
const LEGEND_ITEM_GAP: f64 = 14.0;
const HEADER_BOTTOM_PAD: f64 = 10.0;

/// `layoutXYChart(_:_:)`.
pub fn layout_xy_chart(chart: &XYChart) -> PositionedXYChart {
    if chart.horizontal {
        return layout_horizontal(chart);
    }
    layout_vertical(chart)
}

fn empty_chart() -> PositionedXYChart {
    PositionedXYChart::empty()
}

fn layout_vertical(chart: &XYChart) -> PositionedXYChart {
    let has_title = chart.title.is_some();
    let has_x_title = chart.x_axis.title.is_some();
    let has_y_title = chart.y_axis.title.is_some();
    let has_legend = chart.series.len() > 1;

    let Some(y_range) = chart.y_axis.range else {
        return empty_chart();
    };
    let y_ticks = nice_tick_values(y_range.0, y_range.1);
    let max_y_label_width = max(
        swift::seq_max(
            y_ticks.iter().map(|&v| estimate_text_width(&format_tick_value(v), AXIS_LABEL_FONT_SIZE, AXIS_LABEL_FONT_WEIGHT)),
        )
        .unwrap_or(0.0),
        Y_LABEL_WIDTH,
    );

    let top = PADDING
        + if has_title { TITLE_HEIGHT } else { 0.0 }
        + if has_legend { LEGEND_HEIGHT } else { 0.0 }
        + if has_title || has_legend { HEADER_BOTTOM_PAD } else { 0.0 };
    let bottom = PADDING + X_LABEL_HEIGHT + if has_x_title { AXIS_TITLE_PAD } else { 0.0 };
    let left = PADDING + max_y_label_width + Y_LABEL_GAP + if has_y_title { AXIS_TITLE_PAD } else { 0.0 };
    let right = PADDING;

    let plot_w = PLOT_WIDTH;
    let plot_h = PLOT_HEIGHT;
    let total_w = left + plot_w + right;
    let total_h = top + plot_h + bottom;

    let plot_area = XYPlotArea { x: left, y: top, width: plot_w, height: plot_h };

    let data_count = get_data_count(chart);
    let band_width = plot_w / data_count as f64;
    let x_scale = |i: usize| left + (i as f64 + 0.5) * band_width;
    let y_scale = |v: f64| {
        let t = (v - y_range.0) / if y_range.1 - y_range.0 == 0.0 { 1.0 } else { y_range.1 - y_range.0 };
        top + plot_h - t * plot_h
    };

    let cat_labels = get_category_labels(chart, data_count);
    let x_ticks = build_x_ticks(chart, &x_scale, top + plot_h);

    let y_axis_ticks: Vec<XYAxisTick> = y_ticks
        .iter()
        .map(|&v| XYAxisTick {
            label: format_tick_value(v),
            x: left,
            y: y_scale(v),
            tx: left - TICK_LENGTH,
            ty: y_scale(v),
            label_x: left - Y_LABEL_GAP,
            label_y: y_scale(v),
            text_anchor: "end".into(),
        })
        .collect();

    let grid_lines: Vec<XYGridLine> =
        y_ticks.iter().map(|&v| XYGridLine { x1: left, y1: y_scale(v), x2: left + plot_w, y2: y_scale(v) }).collect();

    let color_map: Vec<i64> = (0..chart.series.len() as i64).collect();

    let bars = layout_bars(chart, &x_scale, &y_scale, band_width, y_range.0, &cat_labels, &color_map);
    let lines = layout_lines(chart, &x_scale, &y_scale, &cat_labels, &color_map);

    let legend_y = PADDING + if has_title { TITLE_HEIGHT } else { 0.0 } + LEGEND_HEIGHT / 2.0;
    let legend = if has_legend { build_legend_items(chart, total_w / 2.0, legend_y, &color_map) } else { vec![] };

    let x_axis_line = AxisLine { x1: left, y1: top + plot_h, x2: left + plot_w, y2: top + plot_h };
    let y_axis_line = AxisLine { x1: left, y1: top, x2: left, y2: top + plot_h };

    let x_axis_obj = PositionedXYAxis {
        title: chart.x_axis.title.as_ref().map(|t| AxisTitle {
            text: t.clone(),
            x: left + plot_w / 2.0,
            y: total_h - PADDING,
            rotate: None,
        }),
        ticks: x_ticks,
        line: x_axis_line,
    };
    let y_axis_obj = PositionedXYAxis {
        title: chart.y_axis.title.as_ref().map(|t| AxisTitle {
            text: t.clone(),
            x: PADDING + 4.0,
            y: top + plot_h / 2.0,
            rotate: Some(-90.0),
        }),
        ticks: y_axis_ticks,
        line: y_axis_line,
    };

    let title_obj = chart.title.as_ref().map(|t| PositionedTitle { text: t.clone(), x: total_w / 2.0, y: PADDING + TITLE_FONT_SIZE });

    PositionedXYChart {
        width: total_w,
        height: total_h,
        horizontal: false,
        title: title_obj,
        x_axis: x_axis_obj,
        y_axis: y_axis_obj,
        plot_area,
        bars,
        lines,
        grid_lines,
        legend,
    }
}

fn layout_horizontal(chart: &XYChart) -> PositionedXYChart {
    let has_title = chart.title.is_some();
    let has_x_title = chart.x_axis.title.is_some();
    let has_y_title = chart.y_axis.title.is_some();
    let has_legend = chart.series.len() > 1;

    let Some(y_range) = chart.y_axis.range else {
        return empty_chart();
    };
    let value_ticks = nice_tick_values(y_range.0, y_range.1);

    let data_count = get_data_count(chart);
    let cat_labels = get_category_labels(chart, data_count);
    let max_cat_label_width = max(
        swift::seq_max(cat_labels.iter().map(|l| estimate_text_width(l, AXIS_LABEL_FONT_SIZE, AXIS_LABEL_FONT_WEIGHT)))
            .unwrap_or(0.0),
        40.0,
    );

    let top = PADDING
        + if has_title { TITLE_HEIGHT } else { 0.0 }
        + if has_legend { LEGEND_HEIGHT } else { 0.0 }
        + if has_title || has_legend { HEADER_BOTTOM_PAD } else { 0.0 };
    let bottom = PADDING + X_LABEL_HEIGHT + if has_y_title { AXIS_TITLE_PAD } else { 0.0 };
    let left = PADDING + max_cat_label_width + Y_LABEL_GAP + if has_x_title { AXIS_TITLE_PAD } else { 0.0 };
    let right = PADDING;

    let plot_w = PLOT_WIDTH;
    let plot_h = PLOT_HEIGHT;
    let total_w = left + plot_w + right;
    let total_h = top + plot_h + bottom;

    let plot_area = XYPlotArea { x: left, y: top, width: plot_w, height: plot_h };

    let value_scale = |v: f64| {
        let t = (v - y_range.0) / if y_range.1 - y_range.0 == 0.0 { 1.0 } else { y_range.1 - y_range.0 };
        left + t * plot_w
    };
    let band_height = plot_h / data_count as f64;
    let cat_scale = |i: usize| top + (i as f64 + 0.5) * band_height;

    let x_ticks: Vec<XYAxisTick> = value_ticks
        .iter()
        .map(|&v| XYAxisTick {
            label: format_tick_value(v),
            x: value_scale(v),
            y: top + plot_h,
            tx: value_scale(v),
            ty: top + plot_h + TICK_LENGTH,
            label_x: value_scale(v),
            label_y: top + plot_h + 18.0,
            text_anchor: "middle".into(),
        })
        .collect();

    let y_ticks: Vec<XYAxisTick> = cat_labels
        .iter()
        .enumerate()
        .map(|(i, label)| XYAxisTick {
            label: label.clone(),
            x: left,
            y: cat_scale(i),
            tx: left - TICK_LENGTH,
            ty: cat_scale(i),
            label_x: left - Y_LABEL_GAP,
            label_y: cat_scale(i),
            text_anchor: "end".into(),
        })
        .collect();

    let grid_lines: Vec<XYGridLine> =
        value_ticks.iter().map(|&v| XYGridLine { x1: value_scale(v), y1: top, x2: value_scale(v), y2: top + plot_h }).collect();

    let color_map: Vec<i64> = (0..chart.series.len() as i64).collect();

    // Bars (horizontal)
    let bar_count = chart.series.iter().filter(|s| s.r#type == XYSeriesType::Bar).count();
    let mut bars: Vec<PositionedBar> = Vec::new();
    if bar_count > 0 {
        let usable = band_height * (1.0 - BAR_PAD_RATIO);
        let raw_bar_h =
            if bar_count > 1 { (usable - (bar_count - 1) as f64 * BAR_GROUP_GAP) / bar_count as f64 } else { usable };
        let single_bar_h = min(raw_bar_h, MAX_BAR_WIDTH);
        let group_h = if bar_count > 1 {
            single_bar_h * bar_count as f64 + BAR_GROUP_GAP * (bar_count - 1) as f64
        } else {
            single_bar_h
        };

        let mut b_idx: i64 = 0;
        for (series_array_idx, s) in chart.series.iter().enumerate() {
            if s.r#type != XYSeriesType::Bar {
                continue;
            }
            for i in 0..min(s.data.len(), cat_labels.len()) {
                let cy = cat_scale(i);
                let group_top = cy - group_h / 2.0;
                let by = group_top + b_idx as f64 * (single_bar_h + BAR_GROUP_GAP);
                let val_x = value_scale(max(s.data[i], y_range.0));
                let base_x = value_scale(max(0.0, y_range.0));
                bars.push(PositionedBar {
                    x: min(base_x, val_x),
                    y: by,
                    width: (val_x - base_x).abs(),
                    height: single_bar_h,
                    value: s.data[i],
                    label: Some(cat_labels[i].clone()),
                    series_index: b_idx,
                    color_index: color_map[series_array_idx],
                });
            }
            b_idx += 1;
        }
    }

    // Lines (horizontal)
    let mut lines: Vec<PositionedLine> = Vec::new();
    let mut line_idx: i64 = 0;
    for (series_idx, s) in chart.series.iter().enumerate() {
        if s.r#type != XYSeriesType::Line {
            continue;
        }
        let points = (0..min(s.data.len(), cat_labels.len()))
            .map(|i| {
                let v = s.data[i];
                LinePoint { x: value_scale(v), y: cat_scale(i), value: v, label: Some(cat_labels[i].clone()) }
            })
            .collect();
        lines.push(PositionedLine { points, series_index: line_idx, color_index: color_map[series_idx] });
        line_idx += 1;
    }

    let x_axis_line = AxisLine { x1: left, y1: top + plot_h, x2: left + plot_w, y2: top + plot_h };
    let y_axis_line = AxisLine { x1: left, y1: top, x2: left, y2: top + plot_h };

    let x_axis_obj = PositionedXYAxis {
        title: chart.y_axis.title.as_ref().map(|t| AxisTitle {
            text: t.clone(),
            x: left + plot_w / 2.0,
            y: total_h - PADDING,
            rotate: None,
        }),
        ticks: x_ticks,
        line: x_axis_line,
    };
    let y_axis_obj = PositionedXYAxis {
        title: chart.x_axis.title.as_ref().map(|t| AxisTitle {
            text: t.clone(),
            x: PADDING + 4.0,
            y: top + plot_h / 2.0,
            rotate: Some(-90.0),
        }),
        ticks: y_ticks,
        line: y_axis_line,
    };

    let title_obj = chart.title.as_ref().map(|t| PositionedTitle { text: t.clone(), x: total_w / 2.0, y: PADDING + TITLE_FONT_SIZE });

    let legend_y = PADDING + if has_title { TITLE_HEIGHT } else { 0.0 } + LEGEND_HEIGHT / 2.0;
    let legend = if has_legend { build_legend_items(chart, total_w / 2.0, legend_y, &color_map) } else { vec![] };

    PositionedXYChart {
        width: total_w,
        height: total_h,
        horizontal: true,
        title: title_obj,
        x_axis: x_axis_obj,
        y_axis: y_axis_obj,
        plot_area,
        bars,
        lines,
        grid_lines,
        legend,
    }
}

/// `_getDataCount(_:)`.
pub fn get_data_count(chart: &XYChart) -> usize {
    if let Some(cats) = &chart.x_axis.categories {
        return cats.len();
    }
    for s in &chart.series {
        if !s.data.is_empty() {
            return s.data.len();
        }
    }
    1
}

/// `_getCategoryLabels(_:_:)`.
pub fn get_category_labels(chart: &XYChart, count: usize) -> Vec<String> {
    if let Some(cats) = &chart.x_axis.categories {
        return cats.clone();
    }
    if let Some(range) = chart.x_axis.range {
        let step = if count > 1 { (range.1 - range.0) / (count - 1) as f64 } else { 0.0 };
        return (0..count).map(|i| format_tick_value(range.0 + step * i as f64)).collect();
    }
    (0..count).map(|i| (i + 1).to_string()).collect()
}

/// `_niceTickValues(_:_:)`.
pub fn nice_tick_values(min: f64, max: f64) -> Vec<f64> {
    let range = max - min;
    if range <= 0.0 {
        return vec![min];
    }

    let raw_interval = range / 6.0;
    let magnitude = libm_pow(10.0, raw_interval.log10().floor());
    let residual = raw_interval / magnitude;
    let nice_interval = if residual <= 1.5 {
        magnitude
    } else if residual <= 3.0 {
        2.0 * magnitude
    } else if residual <= 7.0 {
        5.0 * magnitude
    } else {
        10.0 * magnitude
    };

    let start = (min / nice_interval).ceil() * nice_interval;
    let mut ticks: Vec<f64> = Vec::new();
    let mut v = start;
    while v <= max + nice_interval * 0.001 {
        ticks.push((v * 1e10).round() / 1e10);
        v += nice_interval;
    }
    ticks
}

/// Foundation's `pow` on `Double`: the C library's.
fn libm_pow(x: f64, y: f64) -> f64 {
    upleft_render::swift_compat::pow(x, y)
}

/// `_formatTickValue(_:)`.
pub fn format_tick_value(v: f64) -> String {
    if v == v.round() && v.abs() < 1e15 {
        return swift::int(v).to_string();
    }
    if v.abs() < 10.0 { swift::format_f64("%.1f", v) } else { swift::format_f64("%.0f", v) }
}

fn build_x_ticks(chart: &XYChart, x_scale: &dyn Fn(usize) -> f64, axis_y: f64) -> Vec<XYAxisTick> {
    let count = get_data_count(chart);
    let labels = get_category_labels(chart, count);
    labels
        .into_iter()
        .enumerate()
        .map(|(i, label)| XYAxisTick {
            label,
            x: x_scale(i),
            y: axis_y,
            tx: x_scale(i),
            ty: axis_y + TICK_LENGTH,
            label_x: x_scale(i),
            label_y: axis_y + 18.0,
            text_anchor: "middle".into(),
        })
        .collect()
}

fn layout_bars(
    chart: &XYChart,
    x_scale: &dyn Fn(usize) -> f64,
    y_scale: &dyn Fn(f64) -> f64,
    band_width: f64,
    y_min: f64,
    cat_labels: &[String],
    color_map: &[i64],
) -> Vec<PositionedBar> {
    let bar_count = chart.series.iter().filter(|s| s.r#type == XYSeriesType::Bar).count();
    if bar_count == 0 {
        return vec![];
    }

    let usable = band_width * (1.0 - BAR_PAD_RATIO);
    let raw_bar_w = if bar_count > 1 { (usable - (bar_count - 1) as f64 * BAR_GROUP_GAP) / bar_count as f64 } else { usable };
    let single_bar_w = min(raw_bar_w, MAX_BAR_WIDTH);
    let group_w =
        if bar_count > 1 { single_bar_w * bar_count as f64 + BAR_GROUP_GAP * (bar_count - 1) as f64 } else { single_bar_w };

    let mut bars = Vec::new();
    let mut b_idx: i64 = 0;
    for (series_array_idx, s) in chart.series.iter().enumerate() {
        if s.r#type != XYSeriesType::Bar {
            continue;
        }
        for i in 0..min(s.data.len(), cat_labels.len()) {
            let cx = x_scale(i);
            let group_left = cx - group_w / 2.0;
            let bx = group_left + b_idx as f64 * (single_bar_w + BAR_GROUP_GAP);
            let val_y = y_scale(s.data[i]);
            let base_y = y_scale(max(0.0, y_min));
            bars.push(PositionedBar {
                x: bx,
                y: min(val_y, base_y),
                width: single_bar_w,
                height: (base_y - val_y).abs(),
                value: s.data[i],
                label: Some(cat_labels[i].clone()),
                series_index: b_idx,
                color_index: color_map[series_array_idx],
            });
        }
        b_idx += 1;
    }
    bars
}

fn layout_lines(
    chart: &XYChart,
    x_scale: &dyn Fn(usize) -> f64,
    y_scale: &dyn Fn(f64) -> f64,
    cat_labels: &[String],
    color_map: &[i64],
) -> Vec<PositionedLine> {
    let mut lines = Vec::new();
    let mut line_idx: i64 = 0;
    for (series_array_idx, s) in chart.series.iter().enumerate() {
        if s.r#type != XYSeriesType::Line {
            continue;
        }
        let points = (0..min(s.data.len(), cat_labels.len()))
            .map(|i| {
                let v = s.data[i];
                LinePoint { x: x_scale(i), y: y_scale(v), value: v, label: Some(cat_labels[i].clone()) }
            })
            .collect();
        lines.push(PositionedLine { points, series_index: line_idx, color_index: color_map[series_array_idx] });
        line_idx += 1;
    }
    lines
}

fn build_legend_items(chart: &XYChart, center_x: f64, y: f64, color_map: &[i64]) -> Vec<XYLegendItem> {
    let mut items: Vec<XYLegendItem> = Vec::new();
    let (mut bar_idx, mut line_idx) = (0i64, 0i64);
    for (si, s) in chart.series.iter().enumerate() {
        let label = if s.r#type == XYSeriesType::Bar { format!("Bar {}", bar_idx + 1) } else { format!("Line {}", line_idx + 1) };
        items.push(XYLegendItem {
            label,
            x: 0.0,
            y,
            r#type: s.r#type,
            series_index: if s.r#type == XYSeriesType::Bar { bar_idx } else { line_idx },
            color_index: color_map[si],
        });
        if s.r#type == XYSeriesType::Bar {
            bar_idx += 1;
        } else {
            line_idx += 1;
        }
    }

    let item_widths: Vec<f64> = items
        .iter()
        .map(|item| estimate_text_width(&item.label, LEGEND_FONT_SIZE, LEGEND_FONT_WEIGHT) + LEGEND_SWATCH_W + LEGEND_GAP)
        .collect();
    let total_width = item_widths.iter().fold(0.0, |a, b| a + b) + (items.len() as f64 - 1.0) * LEGEND_ITEM_GAP;
    let mut x = center_x - total_width / 2.0;

    for i in 0..items.len() {
        items[i].x = x;
        x += item_widths[i] + LEGEND_ITEM_GAP;
    }

    items
}
