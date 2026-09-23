# Rust

```rust
#![allow(dead_code)]
use std::collections::HashMap;

/// Doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Parser<'a, T: 'a> {
    input: &'a str,
    items: Vec<T>,
}

const MAX_DEPTH: usize = 0xFF_FF;
static GREETING: &str = "hello\nworld \"quoted\"";

impl<'a, T> Parser<'a, T> where T: Clone {
    pub fn new(input: &'a str) -> Self {
        let raw = r"C:\path\no\escapes";
        let hashed = r#"a "quoted" raw"#;
        let bytes = b"bytes\x00";
        let raw_bytes = br##"raw "#" bytes"##;
        let ch = 'x'; let esc = '\n'; let emoji = '😀'; let quote = '\'';
        let n = 1u8 + 2_000i64 as u8 - 3.5e-2f32 as u8 + 0b1010 + 0o17;
        let range = 0..10; let method = 1.max(2); let float = 1.0.floor();
        /* block /* nested */ still comment */
        match Some(1) { Some(v) if v > 0 => {}, None | Some(_) => {} }
        Self { input, items: Vec::new() }
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let map: HashMap<String, i32> = HashMap::new();
        'outer: loop { break 'outer; }
        unsafe { std::ptr::null::<u8>().read() };
        Ok(())
    }
}

let s = "a string
spanning lines";
let r = r#"unterminated raw
```

```rs
fn main() { println!("{}", 42_u64.pow(2)); }
```
