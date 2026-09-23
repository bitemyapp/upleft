//! `MTMathListIndex.swift`: a path to an atom in a math list.

use std::fmt;

/// The type of the subindex: which branch the path to the atom takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MTMathListSubIndexType {
    /// The index denotes the whole atom, subIndex is nil.
    #[default]
    None = 0,
    /// The position in the subindex is an index into the nucleus
    Nucleus,
    /// The subindex indexes into the superscript.
    Superscript,
    /// The subindex indexes into the subscript
    Subscript,
    /// The subindex indexes into the numerator (only valid for fractions)
    Numerator,
    /// The subindex indexes into the denominator (only valid for fractions)
    Denominator,
    /// The subindex indexes into the radicand (only valid for radicals)
    Radicand,
    /// The subindex indexes into the degree (only valid for radicals)
    Degree,
}

/// An index that points to a particular character in the MTMathList: a linked
/// list of (atom index, branch) steps.
#[derive(Clone, Debug, Hash)]
pub struct MTMathListIndex {
    /// The index of the associated atom.
    pub atom_index: isize,
    /// The type of subindex, e.g. superscript, numerator etc.
    pub sub_index_type: MTMathListSubIndexType,
    /// The index into the sublist.
    pub sub_index: Option<Box<MTMathListIndex>>,
}

impl MTMathListIndex {
    /// `MTMathListIndex(level0Index:)`.
    pub fn level0(index: isize) -> MTMathListIndex {
        MTMathListIndex {
            atom_index: index,
            sub_index_type: MTMathListSubIndexType::None,
            sub_index: None,
        }
    }

    /// `MTMathListIndex(at:with:type:)`.
    pub fn at(
        location: isize,
        sub_index: Option<MTMathListIndex>,
        type_: MTMathListSubIndexType,
    ) -> MTMathListIndex {
        MTMathListIndex {
            atom_index: location,
            sub_index_type: type_,
            sub_index: sub_index.map(Box::new),
        }
    }

    pub fn final_index(&self) -> isize {
        if self.sub_index_type == MTMathListSubIndexType::None {
            self.atom_index
        } else {
            self.sub_index.as_ref().map_or(0, |sub| sub.final_index())
        }
    }

    /// Returns the previous index if present.
    pub fn prev_index(&self) -> Option<MTMathListIndex> {
        if self.sub_index_type == MTMathListSubIndexType::None {
            if self.atom_index > 0 {
                return Some(MTMathListIndex::level0(self.atom_index - 1));
            }
        } else if let Some(prev_sub_index) =
            self.sub_index.as_ref().and_then(|sub| sub.prev_index())
        {
            return Some(MTMathListIndex::at(
                self.atom_index,
                Some(prev_sub_index),
                self.sub_index_type,
            ));
        }
        None
    }

    /// Returns the next index.
    pub fn next_index(&self) -> MTMathListIndex {
        if self.sub_index_type == MTMathListSubIndexType::None {
            MTMathListIndex::level0(self.atom_index + 1)
        } else if self.sub_index_type == MTMathListSubIndexType::Nucleus {
            MTMathListIndex::at(
                self.atom_index + 1,
                self.sub_index.as_deref().cloned(),
                self.sub_index_type,
            )
        } else {
            MTMathListIndex::at(
                self.atom_index,
                self.sub_index.as_ref().map(|sub| sub.next_index()),
                self.sub_index_type,
            )
        }
    }

    /// True if the innermost subindex points to the beginning of a line.
    pub fn is_beginning_of_line(&self) -> bool {
        self.final_index() == 0
    }

    pub fn is_at_same_level(&self, index: Option<&MTMathListIndex>) -> bool {
        if Some(self.sub_index_type) != index.map(|index| index.sub_index_type) {
            false
        } else if self.sub_index_type == MTMathListSubIndexType::None {
            // No subindexes, they are at the same level.
            true
        } else if Some(self.atom_index) != index.map(|index| index.atom_index) {
            false
        } else {
            self.sub_index
                .as_ref()
                .map(|sub| sub.is_at_same_level(index.and_then(|index| index.sub_index.as_deref())))
                .unwrap_or(false)
        }
    }

    /// Returns the type of the innermost sub index.
    pub fn final_sub_index_type(&self) -> MTMathListSubIndexType {
        match &self.sub_index {
            Some(sub) if sub.sub_index.is_some() => sub.final_sub_index_type(),
            _ => self.sub_index_type,
        }
    }

    /// Returns true if any of the subIndexes of this index have the given type.
    pub fn has_sub_index(&self, type_: MTMathListSubIndexType) -> bool {
        if self.sub_index_type == type_ {
            true
        } else {
            self.sub_index
                .as_ref()
                .is_some_and(|sub| sub.has_sub_index(type_))
        }
    }

    pub fn level_up(
        &self,
        sub_index: Option<MTMathListIndex>,
        type_: MTMathListSubIndexType,
    ) -> MTMathListIndex {
        if self.sub_index_type == MTMathListSubIndexType::None {
            return MTMathListIndex::at(self.atom_index, sub_index, type_);
        }
        MTMathListIndex::at(
            self.atom_index,
            self.sub_index
                .as_ref()
                .map(|sub| sub.level_up(sub_index, type_)),
            self.sub_index_type,
        )
    }

    pub fn level_down(&self) -> Option<MTMathListIndex> {
        if self.sub_index_type == MTMathListSubIndexType::None {
            return None;
        }
        if let Some(sub_index_down) = self.sub_index.as_ref().and_then(|sub| sub.level_down()) {
            Some(MTMathListIndex::at(
                self.atom_index,
                Some(sub_index_down),
                self.sub_index_type,
            ))
        } else {
            Some(MTMathListIndex::level0(self.atom_index))
        }
    }
}

impl fmt::Display for MTMathListIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.sub_index {
            Some(sub) => write!(
                f,
                "[{}, {}:{}]",
                self.atom_index, self.sub_index_type as i32, sub
            ),
            None => write!(f, "[{}]", self.atom_index),
        }
    }
}

impl PartialEq for MTMathListIndex {
    fn eq(&self, rhs: &MTMathListIndex) -> bool {
        if self.atom_index != rhs.atom_index || self.sub_index_type != rhs.sub_index_type {
            return false;
        }
        match &rhs.sub_index {
            Some(sub) => self.sub_index.as_deref() == Some(sub),
            None => self.sub_index.is_none(),
        }
    }
}

impl Eq for MTMathListIndex {}
