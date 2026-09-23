//! Port of `alg/layered/p4nodes/bk/ICompactor.swift`.

use super::bk_aligned_layout::BKAlignedLayout;
use crate::prelude::*;

pub trait ICompactor {
    fn horizontal_compaction(&mut self, lg: &LGraphArena, bal: &mut BKAlignedLayout);
}
