//! Hashing.swift — FNV-1a subtree hashing (§3.5).
//!
//! FNV-1a over the block kind, its source bytes and its children's hashes.
//! Seedless, so a hash is comparable with one computed by a previous parse.
//! Bit-identical to Downright's: the decoration cache and `ASTDiff` compare
//! these values.

pub struct FNV;

impl FNV {
    pub const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    pub const PRIME: u64 = 0x100_0000_01b3;

    #[inline(always)]
    pub fn combine_byte(hash: u64, byte: u8) -> u64 {
        (hash ^ byte as u64).wrapping_mul(Self::PRIME)
    }

    /// `combine(_:_: UInt64)`: the value's eight bytes, little end first.
    #[inline(always)]
    pub fn combine_u64(hash: u64, value: u64) -> u64 {
        let mut h = hash;
        let mut v = value;
        for _ in 0..8 {
            h = Self::combine_byte(h, v as u8);
            v >>= 8;
        }
        h
    }

    /// `combine(_:_: String)`: the string's UTF-8 bytes.
    #[inline]
    pub fn combine_str(hash: u64, string: &str) -> u64 {
        let mut h = hash;
        for &byte in string.as_bytes() {
            h = Self::combine_byte(h, byte);
        }
        h
    }

    /// `hash(_: String)`.
    #[inline]
    pub fn hash_str(string: &str) -> u64 {
        Self::combine_str(Self::OFFSET_BASIS, string)
    }

    /// `combine(_:utf16:count:)`: each code unit, low byte first.
    #[inline(always)]
    pub fn combine_utf16(hash: u64, units: &[u16]) -> u64 {
        let mut h = hash;
        for &unit in units {
            h = Self::combine_byte(h, unit as u8);
            h = Self::combine_byte(h, (unit >> 8) as u8);
        }
        h
    }

    /// `combine(_:_: NSString, range:)`: the units of `range`, clamped to the
    /// text's length.
    #[inline]
    pub fn combine_range(hash: u64, text: &[u16], range: crate::NSRange) -> u64 {
        let end = range.upper_bound().min(text.len() as isize);
        if range.location >= end {
            return hash;
        }
        Self::combine_utf16(hash, &text[range.location as usize..end as usize])
    }

    /// `hash(_: NSString, range:)`.
    #[inline]
    pub fn hash_range(text: &[u16], range: crate::NSRange) -> u64 {
        Self::combine_range(Self::OFFSET_BASIS, text, range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_fnv1a() {
        // FNV-1a 64 of "a" and "foobar", from the reference implementation.
        assert_eq!(FNV::hash_str("a"), 0xaf63dc4c8601ec8c);
        assert_eq!(FNV::hash_str("foobar"), 0x85944171f73967e8);
        assert_eq!(FNV::hash_str(""), FNV::OFFSET_BASIS);
    }
}
