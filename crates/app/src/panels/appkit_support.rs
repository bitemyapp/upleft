//! The Swift overlay conveniences the panels lean on, reproduced as the calls
//! they compile to. Nothing here is Downright code: every function is the
//! AppKit, Core Animation or Core Graphics call a Swift one-liner makes
//! (`NSLayoutConstraint.activate([...])`, `layer.actions = [...: NSNull()]`,
//! `CATransaction.begin(); setDisableActions(true); …; commit()`, the
//! accessibility role statics, `NSImage(systemSymbolName:)` with a symbol
//! configuration, and so on), so each panel module reads like the Swift file
//! it ports.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{ClassType, MainThreadMarker, Message, msg_send};
use objc2_app_kit::{
    NSAccessibilityRole, NSColor, NSFont, NSFontWeight, NSFontWeightBold, NSFontWeightMedium,
    NSFontWeightRegular, NSFontWeightSemibold, NSImage, NSImageSymbolConfiguration, NSLayoutConstraint, NSTextField, NSView,
};
use objc2_core_foundation::{CFRetained, CGAffineTransform, CGFloat};
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSArray, NSDictionary, NSNull, NSString};
use objc2_quartz_core::{CALayer, CATransaction};

pub use upleft_render::appkit_compat::{RECT_ZERO, RectExt, main_after, main_async, ns_string, rect, rect_fill};
pub use upleft_render::swift_compat::{smax, smin};

/// `CGAffineTransform.identity`, for the `transform:` argument the Swift
/// overlay's `CGMutablePath.move(to:)`/`addLine(to:)`/`addCurve(to:…)` pass.
pub static IDENTITY: CGAffineTransform = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

/// `NSLayoutConstraint.activate([...])`.
pub fn activate(constraints: &[Retained<NSLayoutConstraint>]) {
    let array = NSArray::from_retained_slice(constraints);
    NSLayoutConstraint::activateConstraints(&array);
}

/// `NSLayoutConstraint.deactivate([...])`.
pub fn deactivate(constraints: &[Retained<NSLayoutConstraint>]) {
    let array = NSArray::from_retained_slice(constraints);
    NSLayoutConstraint::deactivateConstraints(&array);
}

/// `layer.actions = ["key": NSNull(), …]`.
pub fn null_actions(layer: &CALayer, keys: &[&str]) {
    let null = NSNull::null();
    let keys: Vec<Retained<NSString>> = keys.iter().map(|key| NSString::from_str(key)).collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
    let values: Vec<&AnyObject> = keys.iter().map(|_| -> &AnyObject { &null }).collect();
    let dictionary: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&key_refs, &values);
    // SAFETY: an actions dictionary maps keys to `CAAction` objects or
    // `NSNull`, which disables the implicit action.
    unsafe {
        let dictionary: &NSDictionary<NSString, ProtocolObject<dyn objc2_quartz_core::CAAction>> =
            &*(&*dictionary as *const NSDictionary<NSString, AnyObject>
                as *const NSDictionary<NSString, ProtocolObject<dyn objc2_quartz_core::CAAction>>);
        layer.setActions(Some(dictionary));
    }
}

/// `CATransaction.begin(); CATransaction.setDisableActions(true); …;
/// CATransaction.commit()`.
pub fn without_actions<R>(body: impl FnOnce() -> R) -> R {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let result = body();
    CATransaction::commit();
    result
}

/// `color.cgColor`.
pub fn cg(color: &NSColor) -> Retained<CGColor> {
    color.CGColor()
}

/// `[CGColor]` bridged to an `NSArray` (`CAGradientLayer.colors`).
pub fn cg_array(colors: &[Retained<CGColor>]) -> Retained<NSArray<AnyObject>> {
    let objects: Vec<Retained<AnyObject>> = colors
        .iter()
        // SAFETY: `CGColor` is a CF type, and CF types are Objective-C
        // objects when bridged.
        .map(|color| unsafe { Retained::cast_unchecked::<AnyObject>(color.clone()) })
        .collect();
    NSArray::from_retained_slice(&objects)
}

/// A `CGMutablePath` handed out as the `CGPath` Swift returns.
pub fn immutable(path: CFRetained<objc2_core_graphics::CGMutablePath>) -> CFRetained<objc2_core_graphics::CGPath> {
    // SAFETY: a mutable path is a path.
    unsafe { CFRetained::cast_unchecked(path) }
}

/// Any Objective-C object as `&AnyObject`, for `msg_send!`.
pub fn object<T: Message>(target: &T) -> &AnyObject {
    // SAFETY: every `Message` type is an Objective-C object.
    unsafe { &*(target as *const T).cast::<AnyObject>() }
}

/// `NSFont.Weight` constants, read from AppKit's globals.
pub fn weight_regular() -> NSFontWeight {
    // SAFETY: AppKit exports these as immutable globals.
    unsafe { NSFontWeightRegular }
}

pub fn weight_medium() -> NSFontWeight {
    // SAFETY: as above.
    unsafe { NSFontWeightMedium }
}

pub fn weight_semibold() -> NSFontWeight {
    // SAFETY: as above.
    unsafe { NSFontWeightSemibold }
}

pub fn weight_bold() -> NSFontWeight {
    // SAFETY: as above.
    unsafe { NSFontWeightBold }
}

/// `NSImage.SymbolConfiguration(pointSize:weight:)`.
pub fn symbol_configuration(point_size: CGFloat, weight: NSFontWeight) -> Retained<NSImageSymbolConfiguration> {
    NSImageSymbolConfiguration::configurationWithPointSize_weight(point_size, weight)
}

/// `NSImage(systemSymbolName:accessibilityDescription:)`.
pub fn system_symbol(name: &str, description: Option<&str>) -> Option<Retained<NSImage>> {
    let description = description.map(ns_string);
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns_string(name), description.as_deref())
}

/// `NSImage(systemSymbolName:accessibilityDescription:)?.withSymbolConfiguration(configuration)`.
pub fn configured_symbol(
    name: &str,
    description: Option<&str>,
    configuration: &NSImageSymbolConfiguration,
) -> Option<Retained<NSImage>> {
    system_symbol(name, description)?.imageWithSymbolConfiguration(configuration)
}

/// `NSTextField(labelWithString:)`.
pub fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::labelWithString(&ns_string(text), mtm)
}

/// `NSTextField(wrappingLabelWithString:)`.
pub fn wrapping_label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::wrappingLabelWithString(&ns_string(text), mtm)
}

/// The accessibility role statics Swift names as `.group`, `.button`, ….
pub mod role {
    use objc2_app_kit::NSAccessibilityRole;

    macro_rules! role {
        ($name:ident, $symbol:ident) => {
            #[inline]
            pub fn $name() -> &'static NSAccessibilityRole {
                // SAFETY: AppKit exports the roles as immutable globals.
                unsafe { objc2_app_kit::$symbol }
            }
        };
    }

    role!(group, NSAccessibilityGroupRole);
    role!(button, NSAccessibilityButtonRole);
    role!(check_box, NSAccessibilityCheckBoxRole);
    role!(radio_button, NSAccessibilityRadioButtonRole);
    role!(radio_group, NSAccessibilityRadioGroupRole);
    role!(list, NSAccessibilityListRole);
    role!(row, NSAccessibilityRowRole);
    role!(scroll_area, NSAccessibilityScrollAreaRole);
    role!(static_text, NSAccessibilityStaticTextRole);
    role!(text_field, NSAccessibilityTextFieldRole);
    role!(text_area, NSAccessibilityTextAreaRole);
    role!(image, NSAccessibilityImageRole);
    role!(progress_indicator, NSAccessibilityProgressIndicatorRole);
    role!(pop_up_button, NSAccessibilityPopUpButtonRole);
    role!(menu_button, NSAccessibilityMenuButtonRole);
    role!(window, NSAccessibilityWindowRole);
    role!(table, NSAccessibilityTableRole);
    role!(toolbar, NSAccessibilityToolbarRole);
    role!(link, NSAccessibilityLinkRole);
    role!(cell, NSAccessibilityCellRole);
    role!(heading, NSAccessibilityHeadingRole);
    role!(slider, NSAccessibilitySliderRole);
    role!(tab_group, NSAccessibilityTabGroupRole);
    role!(disclosure_triangle, NSAccessibilityDisclosureTriangleRole);
    role!(popover, NSAccessibilityPopoverRole);
    role!(sheet, NSAccessibilitySheetRole);
    role!(layout_area, NSAccessibilityLayoutAreaRole);
    role!(unknown, NSAccessibilityUnknownRole);
}

/// `view.setAccessibilityRole(role)` (any object in the `NSAccessibility`
/// protocol: views, windows, cells).
pub fn set_role<T: Message>(target: &T, role: &NSAccessibilityRole) {
    let _: () = unsafe { msg_send![object(target), setAccessibilityRole: role] };
}

/// `view.setAccessibilityLabel(text)`.
pub fn set_label<T: Message>(target: &T, text: &str) {
    let text = ns_string(text);
    let _: () = unsafe { msg_send![object(target), setAccessibilityLabel: &*text] };
}

/// `view.setAccessibilityHelp(text)`.
pub fn set_help<T: Message>(target: &T, text: &str) {
    let text = ns_string(text);
    let _: () = unsafe { msg_send![object(target), setAccessibilityHelp: &*text] };
}

/// `view.setAccessibilityValue(text)` with a Swift `String` (bridged to
/// `NSString`).
pub fn set_value<T: Message>(target: &T, text: &str) {
    let value = ns_string(text);
    let _: () = unsafe { msg_send![object(target), setAccessibilityValue: &*value] };
}

/// `view.accessibilityLabel()`.
pub fn accessibility_label<T: Message>(target: &T) -> Option<String> {
    let label: Option<Retained<NSString>> = unsafe { msg_send![object(target), accessibilityLabel] };
    label.map(|label| label.to_string())
}

/// `view.toolTip = text`.
pub fn set_tool_tip(view: &NSView, text: Option<&str>) {
    view.setToolTip(text.map(ns_string).as_deref());
}

/// `view is SomeClass` for a class this crate registers (or any class the
/// runtime knows by name). False while the class is not registered yet.
pub fn is_kind_of(target: &AnyObject, class_name: &std::ffi::CStr) -> bool {
    match AnyClass::get(class_name) {
        Some(class) => unsafe { msg_send![target, isKindOfClass: class] },
        None => false,
    }
}

/// `object is T` for a class type.
pub fn is<T: ClassType>(target: &AnyObject) -> bool {
    unsafe { msg_send![target, isKindOfClass: T::class()] }
}

/// `object as? T` for a class type.
pub fn downcast<T: ClassType + Message>(target: &AnyObject) -> Option<Retained<T>> {
    if is::<T>(target) {
        // SAFETY: the object is an instance of `T` or a subclass.
        Some(unsafe { Retained::retain(target as *const AnyObject as *mut T) }.expect("non-null"))
    } else {
        None
    }
}

/// `NSFont.systemFont(ofSize:weight:)`.
pub fn system_font(size: CGFloat, weight: NSFontWeight) -> Retained<NSFont> {
    NSFont::systemFontOfSize_weight(size, weight)
}

/// `-[NSObject respondsToSelector:]` followed by a `CGFloat` getter, for the
/// Swift protocols (`PanelSurface`) the panels cast to.
pub fn cgfloat_if_responds(object: &AnyObject, selector: objc2::runtime::Sel) -> Option<CGFloat> {
    let responds: bool = unsafe { msg_send![object, respondsToSelector: selector] };
    if !responds {
        return None;
    }
    // SAFETY: every class that implements the selector returns a CGFloat.
    Some(unsafe { objc2::runtime::MessageReceiver::send_message(object, selector, ()) })
}

/// Swift's `a === b` on an optional superview: pointer identity, never
/// `-isEqual:`.
pub fn is_same_view(a: Option<Retained<NSView>>, b: &NSView) -> bool {
    a.is_some_and(|a| std::ptr::eq(Retained::as_ptr(&a), b as *const NSView))
}

/// `view.superview` (objc2 marks the getter unsafe: the superview is not
/// retained by the view; Swift's getter returns it strongly).
pub fn superview(view: &NSView) -> Option<Retained<NSView>> {
    // SAFETY: the returned view is retained before use.
    unsafe { view.superview() }
}

/// `layer.mask = mask`.
pub fn set_mask(layer: &CALayer, mask: Option<&CALayer>) {
    // SAFETY: a mask layer must not have a superlayer, which every caller
    // guarantees (a fresh or dedicated mask layer).
    unsafe { layer.setMask(mask) }
}

/// `layer.presentation()`.
pub trait Presentation {
    type Layer;
    fn __presentation(&self) -> Option<Retained<Self::Layer>>;
}

impl Presentation for CALayer {
    type Layer = CALayer;
    fn __presentation(&self) -> Option<Retained<CALayer>> {
        // SAFETY: reading the presentation copy of a layer is always allowed.
        unsafe { self.presentationLayer() }
    }
}

impl Presentation for objc2_quartz_core::CAShapeLayer {
    type Layer = objc2_quartz_core::CAShapeLayer;
    fn __presentation(&self) -> Option<Retained<objc2_quartz_core::CAShapeLayer>> {
        // SAFETY: a shape layer's presentation copy is a shape layer.
        let presentation = unsafe { self.presentationLayer() }?;
        Some(unsafe { Retained::cast_unchecked(presentation) })
    }
}

/// `view.needsDisplay = true` (NSControl shadows the NSView setter with a
/// deprecated no-argument method).
pub fn needs_display(view: &NSView) {
    view.setNeedsDisplay(true);
}
