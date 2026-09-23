//! Swift `OptionSet` structs (`ContentAlignment`, `SizeConstraint`, …) as bit
//! flags. Not an elk-swift file; shared by the option-set modules.

/// Declares an option set with named bit flags and `contains`/`insert`/…
#[macro_export]
macro_rules! option_set {
    ($(#[$m:meta])* $name:ident { $($flag:ident = $bit:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
        pub struct $name(pub i64);

        impl $name {
            $(pub const $flag: $name = $name(1 << $bit);)*

            pub const fn empty() -> $name {
                $name(0)
            }

            pub const fn from_raw(raw: i64) -> $name {
                $name(raw)
            }

            pub const fn raw(self) -> i64 {
                self.0
            }

            pub const fn contains(self, other: $name) -> bool {
                self.0 & other.0 == other.0
            }

            pub fn insert(&mut self, other: $name) {
                self.0 |= other.0;
            }

            pub fn remove(&mut self, other: $name) {
                self.0 &= !other.0;
            }

            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            pub const fn union(self, other: $name) -> $name {
                $name(self.0 | other.0)
            }

            pub const fn intersection(self, other: $name) -> $name {
                $name(self.0 & other.0)
            }

            pub const fn of(flags: &[$name]) -> $name {
                let mut raw = 0;
                let mut i = 0;
                while i < flags.len() {
                    raw |= flags[i].0;
                    i += 1;
                }
                $name(raw)
            }
        }

        impl std::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }
    };
}
