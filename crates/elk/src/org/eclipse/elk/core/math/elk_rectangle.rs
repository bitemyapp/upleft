//! Port of `core/math/ElkRectangle.swift`.

use super::k_vector::KVector;
use crate::swift;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElkRectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ElkRectangle {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> ElkRectangle {
        ElkRectangle { x, y, width, height }
    }

    pub fn set_rect(&mut self, nx: f64, ny: f64, nw: f64, nh: f64) {
        self.x = nx;
        self.y = ny;
        self.width = nw;
        self.height = nh;
    }

    pub fn get_position(&self) -> KVector {
        KVector::new(self.x, self.y)
    }

    pub fn get_top_left(&self) -> KVector {
        self.get_position()
    }

    pub fn get_top_right(&self) -> KVector {
        KVector::new(self.x + self.width, self.y)
    }

    pub fn get_bottom_left(&self) -> KVector {
        KVector::new(self.x, self.y + self.height)
    }

    pub fn get_bottom_right(&self) -> KVector {
        KVector::new(self.x + self.width, self.y + self.height)
    }

    pub fn get_center(&self) -> KVector {
        KVector::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn union(&mut self, other: &ElkRectangle) {
        let mut x1 = swift::min(self.x, other.x);
        let mut y1 = swift::min(self.y, other.y);
        let mut x2 = swift::max(self.x + self.width, other.x + other.width);
        let mut y2 = swift::max(self.y + self.height, other.y + other.height);
        if x2 < x1 {
            std::mem::swap(&mut x1, &mut x2);
        }
        if y2 < y1 {
            std::mem::swap(&mut y1, &mut y2);
        }
        self.set_rect(x1, y1, x2 - x1, y2 - y1);
    }

    pub fn move_by(&mut self, offset: KVector) {
        self.x += offset.x;
        self.y += offset.y;
    }

    pub fn get_max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn get_max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn intersects(&self, rect: &ElkRectangle) -> bool {
        let r1x1 = self.x;
        let r1y1 = self.y;
        let r1x2 = self.x + self.width;
        let r1y2 = self.y + self.height;
        let r2x1 = rect.x;
        let r2y1 = rect.y;
        let r2x2 = rect.x + rect.width;
        let r2y2 = rect.y + rect.height;
        r1x1 < r2x2 && r1x2 > r2x1 && r1y2 > r2y1 && r1y1 < r2y2
    }
}
