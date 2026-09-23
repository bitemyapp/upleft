//! Port of `Mermaid/src_xychart_types.swift` (from `original/src/xychart/types.ts`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XYSeriesType {
    Bar,
    Line,
}

impl XYSeriesType {
    pub fn raw_value(self) -> &'static str {
        match self {
            XYSeriesType::Bar => "bar",
            XYSeriesType::Line => "line",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct XYAxis {
    pub title: Option<String>,
    pub categories: Option<Vec<String>>,
    /// `(min, max)`.
    pub range: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct XYChartSeries {
    pub r#type: XYSeriesType,
    pub data: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct XYChart {
    pub title: Option<String>,
    pub horizontal: bool,
    pub x_axis: XYAxis,
    pub y_axis: XYAxis,
    pub series: Vec<XYChartSeries>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedXYChart {
    pub width: f64,
    pub height: f64,
    pub horizontal: bool,
    pub title: Option<PositionedTitle>,
    pub x_axis: PositionedXYAxis,
    pub y_axis: PositionedXYAxis,
    pub plot_area: XYPlotArea,
    pub bars: Vec<PositionedBar>,
    pub lines: Vec<PositionedLine>,
    pub grid_lines: Vec<XYGridLine>,
    pub legend: Vec<XYLegendItem>,
}

impl PositionedXYChart {
    /// `PositionedXYChart.empty`.
    pub fn empty() -> PositionedXYChart {
        PositionedXYChart {
            width: 0.0,
            height: 0.0,
            horizontal: false,
            title: None,
            x_axis: PositionedXYAxis { title: None, ticks: vec![], line: AxisLine { x1: 0.0, y1: 0.0, x2: 0.0, y2: 0.0 } },
            y_axis: PositionedXYAxis { title: None, ticks: vec![], line: AxisLine { x1: 0.0, y1: 0.0, x2: 0.0, y2: 0.0 } },
            plot_area: XYPlotArea { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
            bars: vec![],
            lines: vec![],
            grid_lines: vec![],
            legend: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedTitle {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedXYAxis {
    pub title: Option<AxisTitle>,
    pub ticks: Vec<XYAxisTick>,
    pub line: AxisLine,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisTitle {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub rotate: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisLine {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct XYAxisTick {
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub tx: f64,
    pub ty: f64,
    pub label_x: f64,
    pub label_y: f64,
    /// "start", "middle", "end".
    pub text_anchor: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XYPlotArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedBar {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub value: f64,
    pub label: Option<String>,
    pub series_index: i64,
    pub color_index: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedLine {
    pub points: Vec<LinePoint>,
    pub series_index: i64,
    pub color_index: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinePoint {
    pub x: f64,
    pub y: f64,
    pub value: f64,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XYGridLine {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct XYLegendItem {
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub r#type: XYSeriesType,
    pub series_index: i64,
    pub color_index: i64,
}
