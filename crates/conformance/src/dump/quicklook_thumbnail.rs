//! The `quicklook-thumbnail` suite: the Rust side of
//! `oracle/app/Sources/downright-app-oracle/QuickLookThumbnailDump.swift`.
//!
//!   quicklook-thumbnail <file.md> <out.png> [--width W] [--height H]
//!       [--scale S] [--layout out.json]
//!
//! `ThumbnailProvider` (upleft-thumb) is asked for a thumbnail through its
//! Objective-C method with an `OracleThumbnailRequest`, and the reply's
//! drawing block draws into the same bitmap the Swift dump draws into. Every
//! call mirrors the Swift file; see it for the format.

use std::cell::RefCell;
use std::rc::Rc;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSGraphicsContext};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSDictionary, NSError, NSURL};
use serde_json::Value;
use upleft_thumb::thumbnail_provider::{QLFileThumbnailRequest, QLThumbnailReply, ThumbnailProvider};

use super::json::{Object, double};
use super::{Failure, Request};

pub struct OracleThumbnailRequestIvars {
    url: Retained<NSURL>,
    size: CGSize,
    request_scale: CGFloat,
}

define_class!(
    /// `OracleThumbnailRequest`: Quick Look creates the real requests itself.
    // SAFETY: the ivars are set before `init`; the overrides keep the
    // superclass's getter signatures.
    #[unsafe(super(QLFileThumbnailRequest, NSObject))]
    #[name = "UpleftOracleThumbnailRequest"]
    #[ivars = OracleThumbnailRequestIvars]
    struct OracleThumbnailRequest;

    impl OracleThumbnailRequest {
        #[unsafe(method_id(fileURL))]
        fn file_url(&self) -> Retained<NSURL> {
            self.ivars().url.clone()
        }

        #[unsafe(method(maximumSize))]
        fn maximum_size(&self) -> CGSize {
            self.ivars().size
        }

        #[unsafe(method(minimumSize))]
        fn minimum_size(&self) -> CGSize {
            CGSize::new(0.0, 0.0)
        }

        #[unsafe(method(scale))]
        fn scale(&self) -> CGFloat {
            self.ivars().request_scale
        }
    }
);

fn flag_value(value: &str, fallback: CGFloat) -> CGFloat {
    value.parse::<f64>().unwrap_or(fallback)
}

pub fn run(request: &Request) -> Result<(), Failure> {
    let mut width: CGFloat = 256.0;
    let mut height: CGFloat = 256.0;
    let mut scale: CGFloat = 2.0;
    let mut layout = None;
    let flags = &request.flags;
    let mut index = 0;
    while index < flags.len() {
        let Some(value) = flags.get(index + 1) else {
            return Err(Failure::Error(format!("{} needs a value", flags[index])));
        };
        match flags[index].as_str() {
            "--width" => width = flag_value(value, 256.0),
            "--height" => height = flag_value(value, 256.0),
            "--scale" => scale = flag_value(value, 2.0),
            "--layout" => layout = Some(std::path::PathBuf::from(value)),
            other => return Err(Failure::Error(format!("unknown flag {other}"))),
        }
        index += 2;
    }

    let provider = ThumbnailProvider::new();
    // `URL(fileURLWithPath:)` of the oracle's input argument.
    let url = upleft_foundation::url::FileUrl::from_path(&request.input.to_string_lossy()).to_nsurl();
    let thumbnail_request = OracleThumbnailRequest::alloc().set_ivars(OracleThumbnailRequestIvars {
        url,
        size: CGSize::new(width, height),
        request_scale: scale,
    });
    // SAFETY: NSObject's initialiser, after the ivars are set.
    let thumbnail_request: Retained<OracleThumbnailRequest> = unsafe { msg_send![super(thumbnail_request), init] };
    type Outcome = (Option<Retained<QLThumbnailReply>>, Option<Retained<NSError>>);
    let result: Rc<RefCell<Option<Outcome>>> = Rc::new(RefCell::new(None));
    let sink = result.clone();
    let handler = RcBlock::new(move |reply: *mut QLThumbnailReply, error: *mut NSError| {
        // SAFETY: the provider hands the handler live objects or nil.
        let outcome = unsafe { (Retained::retain(reply), Retained::retain(error)) };
        *sink.borrow_mut() = Some(outcome);
    });
    let handler_ref: &DynBlock<dyn Fn(*mut QLThumbnailReply, *mut NSError)> = &handler;
    let request_ref: &QLFileThumbnailRequest = &thumbnail_request;
    // SAFETY: the selector and argument types are QLThumbnailProvider's.
    let _: () =
        unsafe { msg_send![&*provider, provideThumbnailForFileRequest: request_ref, completionHandler: handler_ref] };
    let Some((reply, error)) = result.borrow_mut().take() else {
        return Err(Failure::Error("the thumbnail handler was not called before provideThumbnail returned".into()));
    };

    let mut fields = Object::new();
    fields = match &error {
        Some(error) => fields.with(
            "error",
            Object::new().with("domain", error.domain().to_string()).with("code", error.code() as i64).build(),
        ),
        None => fields.with("error", Value::Null),
    };
    let mut png = blank_png()?;
    if let Some(reply) = reply {
        // SAFETY: `contextSize` and `drawingBlock` are QLThumbnailReply's own
        // getters (read by KVC in the Swift dump).
        let context_size: CGSize = unsafe { msg_send![&*reply, contextSize] };
        fields = fields.with("contextSize", Value::Array(vec![double(context_size.width), double(context_size.height)]));
        let block: *mut DynBlock<dyn Fn() -> Bool> = unsafe { msg_send![&*reply, drawingBlock] };
        if block.is_null() {
            return Err(Failure::Error("the reply has no current-context drawing block".into()));
        }
        // SAFETY: the reply keeps the block alive.
        let block = unsafe { &*block };
        let pixel_width = (context_size.width * scale).ceil() as i64;
        let pixel_height = (context_size.height * scale).ceil() as i64;
        if pixel_width > 0 && pixel_height > 0 {
            let (drew, _, data) = render(context_size, scale, || block.call(()).as_bool())?;
            fields = fields.with("drew", drew);
            png = data;
        } else {
            // No bitmap to draw into: the block is not called.
            fields = fields.with("drew", Value::Null);
        }
        fields = fields.with("pixelSize", Value::Array(vec![pixel_width.into(), pixel_height.into()]));
    } else {
        fields = fields.with("contextSize", Value::Null);
    }
    std::fs::write(&request.output, &png)?;
    if let Some(layout) = layout {
        super::json::write(&fields.build(), &layout)?;
    }
    Ok(())
}

/// Draws `draw` as Quick Look draws a current-context reply.
fn render(size: CGSize, scale: CGFloat, draw: impl FnOnce() -> bool) -> Result<(bool, (usize, usize), Vec<u8>), Failure> {
    let width = (size.width * scale).ceil() as usize;
    let height = (size.height * scale).ceil() as usize;
    // SAFETY: `kCGColorSpaceSRGB` is an immutable CoreGraphics global.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or_else(|| Failure::Error("no sRGB colour space".into()))?;
    // SAFETY: a null data pointer lets CoreGraphics allocate the buffer.
    let context = unsafe {
        CGBitmapContextCreate(std::ptr::null_mut(), width, height, 8, 0, Some(&space), CGImageAlphaInfo::PremultipliedLast.0)
    }
    .ok_or_else(|| Failure::Error(format!("cannot create a {width}×{height} bitmap context")))?;
    CGContext::scale_ctm(Some(&context), scale, scale);
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, false)));
    let drew = draw();
    NSGraphicsContext::setCurrentContext(previous.as_deref());
    let image = CGBitmapContextCreateImage(Some(&context)).ok_or_else(|| Failure::Error("PNG encoding failed".into()))?;
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    // SAFETY: an empty property dictionary is valid.
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .ok_or_else(|| Failure::Error("PNG encoding failed".into()))?;
    Ok((drew, (width, height), data.to_vec()))
}

fn blank_png() -> Result<Vec<u8>, Failure> {
    Ok(render(CGSize::new(1.0, 1.0), 1.0, || true)?.2)
}
