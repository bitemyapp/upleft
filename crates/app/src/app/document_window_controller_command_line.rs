//! Port of `App/DocumentWindowController+CommandLine.swift`: applies
//! navigation requested by `down open` after the document's first layout
//! pass has installed its source text view.

use objc2::rc::Weak as ObjcWeak;
use upleft_render::appkit_compat::main_async;

use crate::app::document_window_controller::DocumentWindowController;

impl DocumentWindowController {
    /// `applyCommandLineOpen(line:review:)`.
    pub fn apply_command_line_open(&self, line: Option<isize>, review: bool) {
        if line.is_none() && !review {
            return;
        }
        let weak = ObjcWeak::new(self);
        // `Task { @MainActor [weak self] in … }`
        main_async(move || {
            let Some(this) = weak.load() else { return };
            if let Some(line) = line {
                let text = this.markdown_document().storage().string();
                let length = text.length() as isize;
                // `text.components(separatedBy: "\n")`, as UTF-16 lengths.
                let mut units = vec![0u16; length as usize];
                if let Some(buffer) = std::ptr::NonNull::new(units.as_mut_ptr()) {
                    // SAFETY: the buffer holds exactly `length` code units.
                    unsafe { text.getCharacters_range(buffer, objc2_foundation::NSRange::new(0, length as usize)) };
                }
                let lines: Vec<isize> = units.split(|unit| *unit == 0x0A).map(|piece| piece.len() as isize).collect();
                let target_line = (1.max(line) - 1).min(0.max(lines.len() as isize - 1));
                let mut offset = 0isize;
                for index in 0..target_line {
                    offset += lines[index as usize] + 1;
                }
                this.jump(offset.min(length), &format!("Line {line}"), false);
            }
            if review {
                this.ensure_review_panel_visible();
            }
        });
    }
}
