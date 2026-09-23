//! Port of `alg/common/nodespacing/cellsystem/Cell.swift`.
//!
//! A cell is the basic component of the cell system. Each cell has a padding,
//! which determines the amount of space between its content area and its
//! border. It also has a minimum width and height, which is the minimum size it
//! would like to be in the final layout. Container cells use that information
//! to compute their own minimum size. Whether or not a cell contributes to that
//! is controlled through its flags, for width and height separately. Finally,
//! a cell has a rectangle which describes its actual position and size.
//!
//! Swift's cells are class instances shared by reference: a node context keeps
//! a label cell both in its `nodeLabelCells` map and in a container, and an
//! inside port label cell both in `insidePortLabelCells` and in a strip. The
//! port keeps every cell in an `Rc<RefCell<_>>`; a container's child slot is a
//! [`CellRef`], an enum over the concrete cell types that stands in for a
//! Swift `Cell` reference (dynamic dispatch of `getMinimumWidth()` and the
//! `as? AtomicCell` / `as? ContainerCell` downcasts).

use super::atomic_cell::AtomicCellRef;
use super::grid_container_cell::GridContainerCellRef;
use super::label_cell::LabelCellRef;
use super::strip_container_cell::StripContainerCellRef;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;

/// The state of the Swift `Cell` base class.
#[derive(Clone, Debug, Default)]
pub struct Cell {
    /// A cell has a padding.
    pub padding: ElkPadding,
    /// The actual size and position of the cell. Includes the padding.
    pub cell_rectangle: ElkRectangle,
    /// Whether the cell contributes to the minimum width calculation of a container cell or not.
    pub contributes_to_minimum_width: bool,
    /// Whether the cell contributes to the minimum height calculation of a container cell or not.
    pub contributes_to_minimum_height: bool,
}

impl Cell {
    /// `getPadding()`: the cell's padding, to be modified by the caller.
    pub fn get_padding(&mut self) -> &mut ElkPadding {
        &mut self.padding
    }

    /// `getCellRectangle()`: the cell's rectangle, to be modified by the caller.
    pub fn get_cell_rectangle(&mut self) -> &mut ElkRectangle {
        &mut self.cell_rectangle
    }

    pub fn is_contributing_to_minimum_width(&self) -> bool {
        self.contributes_to_minimum_width
    }

    pub fn set_contributes_to_minimum_width(&mut self, contributes_to_minimum_width: bool) {
        self.contributes_to_minimum_width = contributes_to_minimum_width;
    }

    pub fn is_contributing_to_minimum_height(&self) -> bool {
        self.contributes_to_minimum_height
    }

    pub fn set_contributes_to_minimum_height(&mut self, contributes_to_minimum_height: bool) {
        self.contributes_to_minimum_height = contributes_to_minimum_height;
    }
}

/// A reference to any cell (Swift `Cell` class reference).
#[derive(Clone, Debug)]
pub enum CellRef {
    Atomic(AtomicCellRef),
    Label(LabelCellRef),
    Strip(StripContainerCellRef),
    Grid(GridContainerCellRef),
}

impl CellRef {
    /// Runs `f` on the cell's base state.
    pub fn with_cell<R>(&self, f: impl FnOnce(&Cell) -> R) -> R {
        match self {
            CellRef::Atomic(c) => f(&c.borrow().cell),
            CellRef::Label(c) => f(&c.borrow().cell),
            CellRef::Strip(c) => f(&c.borrow().cell),
            CellRef::Grid(c) => f(&c.borrow().cell),
        }
    }

    /// Runs `f` on the cell's base state, mutably.
    pub fn with_cell_mut<R>(&self, f: impl FnOnce(&mut Cell) -> R) -> R {
        match self {
            CellRef::Atomic(c) => f(&mut c.borrow_mut().cell),
            CellRef::Label(c) => f(&mut c.borrow_mut().cell),
            CellRef::Strip(c) => f(&mut c.borrow_mut().cell),
            CellRef::Grid(c) => f(&mut c.borrow_mut().cell),
        }
    }

    /// `getMinimumWidth()` (dynamically dispatched).
    pub fn get_minimum_width(&self) -> f64 {
        match self {
            CellRef::Atomic(c) => c.borrow().get_minimum_width(),
            CellRef::Label(c) => c.borrow().get_minimum_width(),
            CellRef::Strip(c) => c.borrow().get_minimum_width(),
            CellRef::Grid(c) => c.borrow().get_minimum_width(),
        }
    }

    /// `getMinimumHeight()` (dynamically dispatched).
    pub fn get_minimum_height(&self) -> f64 {
        match self {
            CellRef::Atomic(c) => c.borrow().get_minimum_height(),
            CellRef::Label(c) => c.borrow().get_minimum_height(),
            CellRef::Strip(c) => c.borrow().get_minimum_height(),
            CellRef::Grid(c) => c.borrow().get_minimum_height(),
        }
    }

    pub fn is_contributing_to_minimum_width(&self) -> bool {
        self.with_cell(|c| c.contributes_to_minimum_width)
    }

    pub fn is_contributing_to_minimum_height(&self) -> bool {
        self.with_cell(|c| c.contributes_to_minimum_height)
    }

    /// `cell as? AtomicCell`.
    pub fn as_atomic(&self) -> Option<&AtomicCellRef> {
        match self {
            CellRef::Atomic(c) => Some(c),
            _ => None,
        }
    }

    /// `if let containerCell = cell as? ContainerCell { containerCell.layoutChildrenHorizontally() }`.
    pub fn layout_children_horizontally_if_container(&self) {
        match self {
            CellRef::Strip(c) => c.borrow_mut().layout_children_horizontally(),
            CellRef::Grid(c) => c.borrow_mut().layout_children_horizontally(),
            _ => {}
        }
    }

    /// `if let containerCell = cell as? ContainerCell { containerCell.layoutChildrenVertically() }`.
    pub fn layout_children_vertically_if_container(&self) {
        match self {
            CellRef::Strip(c) => c.borrow_mut().layout_children_vertically(),
            CellRef::Grid(c) => c.borrow_mut().layout_children_vertically(),
            _ => {}
        }
    }
}
