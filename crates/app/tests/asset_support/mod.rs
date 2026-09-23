//! Helpers shared by the asset test ports (`DocumentDropTests`,
//! `ShareAndCaptureTests`): the Swift suites' bitmap and named-pasteboard
//! helpers. Named pasteboards never touch the user's general pasteboard.
#![allow(dead_code)]

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSGraphicsContext, NSPasteboard,
    NSPasteboardType, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use upleft_foundation::url::FileUrl;

/// The Swift suites' `bitmap(width:height:)`: an RGBA bitmap filled with
/// `systemTeal`.
pub fn bitmap(width: isize, height: isize) -> Retained<NSBitmapImageRep> {
    // SAFETY: AppKit allocates the planes (`nil`); the color space is an
    // AppKit constant.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            width,
            height,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .expect("bitmap");
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep).as_deref());
    NSColor::systemTealColor().setFill();
    upleft_render::appkit_compat::rect_fill(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width as f64, height as f64)));
    NSGraphicsContext::restoreGraphicsState_class();
    rep
}

/// `rep.representation(using:properties: [:])`.
pub fn representation(rep: &NSBitmapImageRep, file_type: NSBitmapImageFileType) -> Option<Vec<u8>> {
    // SAFETY: an empty properties dictionary.
    unsafe { rep.representationUsingType_properties(file_type, &NSDictionary::new()) }.map(|data| data.to_vec())
}

/// The DocumentDropTests `pngData()` helper (8 × 8).
pub fn png_data() -> Vec<u8> {
    representation(&bitmap(8, 8), NSBitmapImageFileType::PNG).expect("png")
}

/// A named pasteboard that is released globally when dropped.
pub struct Board(pub Retained<NSPasteboard>);

impl Drop for Board {
    fn drop(&mut self) {
        // SAFETY: `-releaseGlobally` takes no arguments.
        let _: () = unsafe { msg_send![&*self.0, releaseGlobally] };
    }
}

impl std::ops::Deref for Board {
    type Target = NSPasteboard;
    fn deref(&self) -> &NSPasteboard {
        &self.0
    }
}

/// `NSPasteboard(name: "<prefix>-<UUID>")`, cleared.
pub fn named_pasteboard(prefix: &str) -> Board {
    let name = NSString::from_str(&format!("{prefix}-{}", upleft_foundation::foundation_io::uuid_string()));
    let board = NSPasteboard::pasteboardWithName(&name);
    board.clearContents();
    Board(board)
}

/// `board.setData(data, forType: type)`.
pub fn set_data(board: &NSPasteboard, data: &[u8], pasteboard_type: &NSPasteboardType) {
    board.setData_forType(Some(&NSData::with_bytes(data)), pasteboard_type);
}

/// `board.writeObjects(files.map { $0 as NSURL })`.
pub fn write_files(board: &NSPasteboard, files: &[FileUrl]) {
    let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        files.iter().map(|file| ProtocolObject::from_retained(file.to_nsurl())).collect();
    board.writeObjects(&NSArray::from_retained_slice(&objects));
}

/// `NSPasteboard.PasteboardType(raw)`.
pub fn pasteboard_type(raw: &str) -> Retained<NSPasteboardType> {
    NSString::from_str(raw)
}
