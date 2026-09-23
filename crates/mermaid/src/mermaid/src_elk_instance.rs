//! Port of `Mermaid/src_elk_instance.swift` (from `original/src/elk-instance.ts`):
//! the one entry point into ELK. The Swift file also carries a private
//! fallback layered layout (`_layoutRecursively`) that nothing calls; it is
//! not ported.
//!
//! The engine is `upleft-elk` (the port of elk-swift). Until that crate is
//! merged, the `elk` feature is off and every call fails with
//! [`MermaidError::Elk`], which callers surface as "not available".

use serde_json::Value;

use crate::error::MermaidError;

/// `ElkNode`: a JSON dictionary (`[String: Any]` in Swift).
pub type ElkNode = Value;

/// `elkLayoutSync(_:)`.
pub fn elk_layout_sync(graph: &ElkNode) -> Result<ElkNode, MermaidError> {
    // `_validateElkGraph`: the graph needs a non-empty string id.
    match graph.get("id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => {}
        _ => return Err(MermaidError::Elk("ELK graph must contain a string 'id' field.".into())),
    }
    engine::layout(graph)
}

/// Whether an ELK engine is linked in.
pub fn elk_available() -> bool {
    engine::AVAILABLE
}

#[cfg(feature = "elk")]
mod engine {
    use serde_json::Value;

    use crate::error::MermaidError;

    pub const AVAILABLE: bool = true;

    thread_local! {
        /// `_ElkBridgeRuntime.shared()`: one engine, created on first use.
        static ELK: std::cell::RefCell<upleft_elk::bridge::elk::Elk> =
            std::cell::RefCell::new(upleft_elk::bridge::elk::Elk::new());
    }

    pub fn layout(graph: &Value) -> Result<Value, MermaidError> {
        ELK.with(|elk| elk.borrow_mut().layout(graph)).map_err(|e| MermaidError::Elk(e.to_string()))
    }
}

#[cfg(not(feature = "elk"))]
mod engine {
    use serde_json::Value;

    use crate::error::MermaidError;

    pub const AVAILABLE: bool = false;

    pub fn layout(_graph: &Value) -> Result<Value, MermaidError> {
        Err(MermaidError::Elk("ELK layout unavailable: upleft-elk is not linked".into()))
    }
}
