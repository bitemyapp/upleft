//! Port of `core/options/TopdownNodeTypes.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TopdownNodeTypes {
    PARALLEL_NODE,
    HIERARCHICAL_NODE,
    ROOT_NODE,
}
