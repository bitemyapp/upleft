//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/org_eclipse_elk_alg_layered_p3order_counting_BinaryIndexedTree.swift`.
//!
//! A Fenwick tree over the indices `0..<maxNum` counting how many times each
//! index was added. Indices are Swift `Int`s; a negative or too large index
//! traps (panics) exactly where the Swift array access would.

#[derive(Clone, Debug)]
pub struct BinaryIndexedTree {
    pub binary_sums: Vec<i64>,
    pub nums_per_index: Vec<i64>,
    pub size_value: i64,
    pub max_num: usize,
}

#[inline]
fn ix(i: i64) -> usize {
    // A negative Swift index traps; `as usize` makes it huge so indexing panics.
    i as usize
}

impl BinaryIndexedTree {
    pub fn new(max_num: usize) -> BinaryIndexedTree {
        BinaryIndexedTree { binary_sums: vec![0; max_num + 1], nums_per_index: vec![0; max_num], size_value: 0, max_num }
    }

    /// `add(_:)`: increments the count at `index`.
    pub fn add(&mut self, index: i64) {
        self.size_value += 1;
        self.nums_per_index[ix(index)] += 1;
        let mut i = index + 1;
        while i < self.binary_sums.len() as i64 {
            self.binary_sums[i as usize] += 1;
            i += i & -i;
        }
    }

    /// `rank(_:)`: how many added entries are smaller than `index`.
    pub fn rank(&self, index: i64) -> i64 {
        let mut i = index;
        let mut sum = 0;
        while i > 0 {
            sum += self.binary_sums[ix(i)];
            i -= i & -i;
        }
        sum
    }

    pub fn size(&self) -> i64 {
        self.size_value
    }

    /// `removeAll(_:)`: removes every entry at `index`.
    pub fn remove_all(&mut self, index: i64) {
        let num_entries = self.nums_per_index[ix(index)];
        if num_entries == 0 {
            return;
        }
        self.nums_per_index[ix(index)] = 0;
        self.size_value -= num_entries;
        let mut i = index + 1;
        while i < self.binary_sums.len() as i64 {
            self.binary_sums[i as usize] -= num_entries;
            i += i & -i;
        }
    }

    /// `clear()` (Swift reallocates the arrays; zero-filling is the same state).
    pub fn clear(&mut self) {
        self.binary_sums.iter_mut().for_each(|v| *v = 0);
        self.nums_per_index.iter_mut().for_each(|v| *v = 0);
        self.size_value = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.size_value == 0
    }
}
