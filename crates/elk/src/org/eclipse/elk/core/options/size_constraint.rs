//! Port of `core/options/SizeConstraint.swift`.

crate::option_set!(SizeConstraint {
    PORTS = 0,
    PORT_LABELS = 1,
    NODE_LABELS = 2,
    MINIMUM_SIZE = 3,
});

impl SizeConstraint {
    pub const fn fixed() -> SizeConstraint {
        SizeConstraint(0)
    }

    pub const fn minimum_size_with_ports() -> SizeConstraint {
        SizeConstraint::of(&[SizeConstraint::PORTS, SizeConstraint::MINIMUM_SIZE])
    }

    pub const fn free() -> SizeConstraint {
        SizeConstraint::of(&[SizeConstraint::PORTS, SizeConstraint::PORT_LABELS, SizeConstraint::NODE_LABELS, SizeConstraint::MINIMUM_SIZE])
    }
}
