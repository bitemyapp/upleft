//! Port of `alg/layered/components/ComponentOrderingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ComponentOrderingStrategy {
    NONE,
    INSIDE_PORT_SIDE_GROUPS,
    GROUP_MODEL_ORDER,
    MODEL_ORDER,
}
