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
    if let Some(result) = replay::next(graph) {
        return result;
    }
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

/// Test support: answer `elkLayoutSync` calls from a record of Swift's
/// (crates/mermaid/tools/elk-capture) instead of an engine, in call order,
/// and keep the graphs this port handed over so they can be compared with
/// the ones Swift built. Per thread; nothing is installed by default.
pub mod replay {
    use std::cell::RefCell;

    use serde_json::Value;

    use crate::error::MermaidError;

    struct Replay {
        outputs: Vec<Option<Value>>,
        inputs: Vec<Value>,
    }

    thread_local! {
        static STATE: RefCell<Option<Replay>> = const { RefCell::new(None) };
    }

    /// Answer the next calls with `outputs` (`None`: the call failed).
    pub fn install(outputs: Vec<Option<Value>>) {
        STATE.with(|s| *s.borrow_mut() = Some(Replay { outputs, inputs: Vec::new() }));
    }

    /// Stop replaying; returns the graphs handed to ELK meanwhile.
    pub fn finish() -> Vec<Value> {
        STATE.with(|s| s.borrow_mut().take().map(|r| r.inputs).unwrap_or_default())
    }

    pub(super) fn next(graph: &Value) -> Option<Result<Value, MermaidError>> {
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            let replay = state.as_mut()?;
            let index = replay.inputs.len();
            replay.inputs.push(graph.clone());
            Some(match replay.outputs.get(index).cloned().flatten() {
                Some(output) => Ok(output),
                None => Err(MermaidError::Elk("the recorded layout failed or is missing".into())),
            })
        })
    }
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
