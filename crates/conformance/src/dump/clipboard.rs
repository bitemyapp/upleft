//! Rust counterpart of `oracle/Sources/downright-oracle/ClipboardDump.swift`
//! (`clipboard`): `ClipboardSemanticHTML.render(markdown:)` of the document.

use upleft_render::clipboard_semantic_html::ClipboardSemanticHTML;

use super::json::Object;
use super::{Failure, Request};

pub fn run(request: &Request) -> Result<(), Failure> {
    let text = super::markup::read_text(&request.input)?;
    let value = Object::new().with("html", ClipboardSemanticHTML::render(&text)).build();
    super::json::write(&value, &request.output)?;
    Ok(())
}
