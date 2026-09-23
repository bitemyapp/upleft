//! Port of `core/options/ContentAlignment.swift`.

crate::option_set!(ContentAlignment {
    V_TOP = 0,
    V_CENTER = 1,
    V_BOTTOM = 2,
    H_LEFT = 4,
    H_CENTER = 5,
    H_RIGHT = 6,
});

impl ContentAlignment {
    pub fn vertical(self) -> ContentAlignment {
        ContentAlignment(self.0 & 0b0111)
    }

    pub fn horizontal(self) -> ContentAlignment {
        ContentAlignment(self.0 & 0b1110000)
    }
}
