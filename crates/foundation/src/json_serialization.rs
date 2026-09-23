//! `JSONSerialization`, called through objc2, with a Rust mirror of the
//! `Any` trees Swift code builds and reads.
//!
//! Downright reads and writes agent settings files with `JSONSerialization`,
//! so its output (key order under `.sortedKeys`, which is `NSString`
//! comparison and not Swift's `<`; 17-digit doubles; escaping) comes from
//! Foundation itself here too, never from a Rust reimplementation.
//!
//! Numbers read from JSON keep the `NSNumber` Foundation created
//! ([`AnyJson::Number`]), so an integer stays an integer and a double keeps
//! its exact value and type when it is written back. Booleans come back as
//! [`AnyJson::Bool`] (Foundation reads `true`/`false` as the `CFBoolean`
//! singletons).

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::AnyClass;
use objc2::{ClassType, Message, msg_send};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSJSONReadingOptions, NSJSONSerialization, NSJSONWritingOptions, NSMutableArray,
    NSMutableDictionary, NSNull, NSNumber, NSString,
};

/// A value in a `[String: Any]` / `[Any]` tree as `JSONSerialization` reads
/// and writes it.
#[derive(Clone, Debug)]
pub enum AnyJson {
    /// `NSNull()`.
    Null,
    /// A Swift `Bool`, bridged to `kCFBooleanTrue` / `kCFBooleanFalse`.
    Bool(bool),
    /// A Swift `Int`, bridged to an `NSNumber` holding a `long long`.
    Int(i64),
    /// A Swift `Double`, bridged to an `NSNumber` holding a `double`.
    Double(f64),
    /// A number exactly as Foundation read it from JSON.
    Number(Retained<NSNumber>),
    String(String),
    Array(Vec<AnyJson>),
    /// Members in the order Foundation enumerated them (for a parsed file,
    /// hash order; for a tree built in Rust, insertion order).
    Object(Vec<(String, AnyJson)>),
}

impl PartialEq for AnyJson {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AnyJson::Null, AnyJson::Null) => true,
            (AnyJson::Bool(a), AnyJson::Bool(b)) => a == b,
            (AnyJson::String(a), AnyJson::String(b)) => a == b,
            (AnyJson::Array(a), AnyJson::Array(b)) => a == b,
            (AnyJson::Object(a), AnyJson::Object(b)) => {
                a.len() == b.len() && a.iter().all(|(key, value)| b.iter().any(|(k, v)| k == key && v == value))
            }
            (a, b) => match (a.number(), b.number()) {
                // `NSNumber.isEqual`: compares values across types.
                (Some(x), Some(y)) => x.isEqualToNumber(&y),
                _ => false,
            },
        }
    }
}

/// `JSONSerialization.WritingOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WritingOptions {
    pub pretty_printed: bool,
    pub sorted_keys: bool,
    pub fragments_allowed: bool,
    pub without_escaping_slashes: bool,
}

impl WritingOptions {
    fn ns(self) -> NSJSONWritingOptions {
        let mut options = NSJSONWritingOptions::empty();
        if self.pretty_printed {
            options |= NSJSONWritingOptions::PrettyPrinted;
        }
        if self.sorted_keys {
            options |= NSJSONWritingOptions::SortedKeys;
        }
        if self.fragments_allowed {
            options |= NSJSONWritingOptions::FragmentsAllowed;
        }
        if self.without_escaping_slashes {
            options |= NSJSONWritingOptions::WithoutEscapingSlashes;
        }
        options
    }
}

/// `JSONSerialization.ReadingOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadingOptions {
    pub fragments_allowed: bool,
}

impl AnyJson {
    /// `value as? [String: Any]`.
    pub fn as_object(&self) -> Option<&Vec<(String, AnyJson)>> {
        match self {
            AnyJson::Object(members) => Some(members),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Vec<(String, AnyJson)>> {
        match self {
            AnyJson::Object(members) => Some(members),
            _ => None,
        }
    }

    /// `value as? [Any]`.
    pub fn as_array(&self) -> Option<&Vec<AnyJson>> {
        match self {
            AnyJson::Array(values) => Some(values),
            _ => None,
        }
    }

    /// `value as? String`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AnyJson::String(text) => Some(text),
            _ => None,
        }
    }

    /// `dictionary[key]` on an object.
    pub fn get(&self, key: &str) -> Option<&AnyJson> {
        self.as_object()?.iter().find(|(k, _)| k == key).map(|(_, value)| value)
    }

    /// `dictionary[key] = value` on an object: replaces in place, or appends.
    pub fn set(members: &mut Vec<(String, AnyJson)>, key: &str, value: AnyJson) {
        match members.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => members.push((key.to_owned(), value)),
        }
    }

    /// `dictionary.removeValue(forKey:)`.
    pub fn remove(members: &mut Vec<(String, AnyJson)>, key: &str) -> Option<AnyJson> {
        let index = members.iter().position(|(k, _)| k == key)?;
        Some(members.remove(index).1)
    }

    /// The `NSNumber` a number (or Boolean) bridges to.
    pub fn number(&self) -> Option<Retained<NSNumber>> {
        match self {
            AnyJson::Bool(flag) => Some(NSNumber::numberWithBool(*flag)),
            AnyJson::Int(value) => Some(NSNumber::numberWithLongLong(*value)),
            AnyJson::Double(value) => Some(NSNumber::numberWithDouble(*value)),
            AnyJson::Number(number) => Some(number.clone()),
            _ => None,
        }
    }

    /// Bridges the tree to Foundation objects, as Swift does when it passes
    /// an `Any` to an Objective-C API.
    pub fn to_foundation(&self) -> Retained<AnyObject> {
        match self {
            AnyJson::Null => Retained::into_super(Retained::into_super(NSNull::null())),
            AnyJson::String(text) => Retained::into_super(Retained::into_super(NSString::from_str(text))),
            AnyJson::Array(values) => {
                let array = NSMutableArray::<AnyObject>::new();
                for value in values {
                    array.addObject(&value.to_foundation());
                }
                Retained::into_super(Retained::into_super(Retained::into_super(array)))
            }
            AnyJson::Object(members) => {
                let dictionary = NSMutableDictionary::<NSString, AnyObject>::new();
                for (key, value) in members {
                    let key = NSString::from_str(key);
                    dictionary.insert(&*key, &value.to_foundation());
                }
                Retained::into_super(Retained::into_super(Retained::into_super(dictionary)))
            }
            number => {
                let number = number.number().expect("number");
                Retained::into_super(Retained::into_super(Retained::into_super(number)))
            }
        }
    }

    /// Reads a Foundation object tree (what `JSONObjectWithData` returns).
    ///
    /// # Safety
    ///
    /// `object` must be an `NSDictionary`, `NSArray`, `NSString`, `NSNumber`
    /// or `NSNull` tree whose dictionary keys are strings.
    pub unsafe fn from_foundation(object: &AnyObject) -> AnyJson {
        if is_kind_of(object, NSDictionary::<AnyObject, AnyObject>::class()) {
            let dictionary: &NSDictionary<NSString, AnyObject> = unsafe { &*(object as *const AnyObject).cast() };
            let (keys, values) = dictionary.to_vecs();
            AnyJson::Object(
                keys.iter()
                    .zip(values.iter())
                    .map(|(key, value)| (key.to_string(), unsafe { AnyJson::from_foundation(value) }))
                    .collect(),
            )
        } else if is_kind_of(object, NSArray::<AnyObject>::class()) {
            let array: &NSArray<AnyObject> = unsafe { &*(object as *const AnyObject).cast() };
            AnyJson::Array(array.to_vec().iter().map(|value| unsafe { AnyJson::from_foundation(value) }).collect())
        } else if is_kind_of(object, NSString::class()) {
            let string: &NSString = unsafe { &*(object as *const AnyObject).cast() };
            AnyJson::String(string.to_string())
        } else if is_kind_of(object, NSNumber::class()) {
            let number: &NSNumber = unsafe { &*(object as *const AnyObject).cast() };
            if is_cf_boolean(number) {
                AnyJson::Bool(number.boolValue())
            } else {
                AnyJson::Number(number.retain())
            }
        } else {
            AnyJson::Null
        }
    }
}

fn is_kind_of(object: &AnyObject, class: &AnyClass) -> bool {
    unsafe { msg_send![object, isKindOfClass: class] }
}

/// Whether `number` is one of the `kCFBooleanTrue` / `kCFBooleanFalse`
/// singletons (Swift's `Bool` bridge and JSON's `true`/`false`).
pub fn is_cf_boolean(number: &NSNumber) -> bool {
    let yes = NSNumber::numberWithBool(true);
    let no = NSNumber::numberWithBool(false);
    std::ptr::eq(number, &*yes) || std::ptr::eq(number, &*no)
}

/// The text of an `NSError` as Swift's `error.localizedDescription` reads it.
fn describe(error: &objc2_foundation::NSError) -> String {
    error.localizedDescription().to_string()
}

/// `JSONSerialization.data(withJSONObject:options:)`.
///
/// Returns `Err` where Swift throws. An invalid top-level object (which
/// Foundation reports by raising an Objective-C exception, crashing a Swift
/// caller) is refused up front with `isValidJSONObject`.
pub fn data(object: &AnyJson, options: WritingOptions) -> Result<Vec<u8>, String> {
    let foundation = object.to_foundation();
    if !options.fragments_allowed && !unsafe { NSJSONSerialization::isValidJSONObject(&foundation) } {
        return Err("Invalid top-level type in JSON write".into());
    }
    let data = unsafe { NSJSONSerialization::dataWithJSONObject_options_error(&foundation, options.ns()) }
        .map_err(|error| describe(&error))?;
    Ok(data.to_vec())
}

/// `JSONSerialization.jsonObject(with:options:)`.
pub fn json_object(bytes: &[u8], options: ReadingOptions) -> Result<AnyJson, String> {
    let data = NSData::with_bytes(bytes);
    let mut ns_options = NSJSONReadingOptions::empty();
    if options.fragments_allowed {
        ns_options |= NSJSONReadingOptions::FragmentsAllowed;
    }
    let object =
        NSJSONSerialization::JSONObjectWithData_options_error(&data, ns_options).map_err(|error| describe(&error))?;
    Ok(unsafe { AnyJson::from_foundation(&object) })
}

/// `NSNumber` accessors a caller may need on [`AnyJson::Number`].
pub fn number_objc_type(number: &NSNumber) -> String {
    let pointer = number.objCType();
    unsafe { std::ffi::CStr::from_ptr(pointer.as_ptr()) }.to_string_lossy().into_owned()
}

/// `(number as NSNumber).stringValue`.
pub fn number_string_value(number: &NSNumber) -> String {
    let text: Retained<NSString> = unsafe { msg_send![number, stringValue] };
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Swift 6.4, macOS 26:
    //   let any: [String: Any] = ["s": "a/b\u{2028}é\u{1}", "d": 0.1, "i": 3, "b": true, "arr": [],
    //                             "obj": [String: Any](), "n": NSNull(), "big": 1e16, "f": 1.5]
    //   JSONSerialization.data(withJSONObject: any, options: [.sortedKeys])
    #[test]
    fn writes_what_foundation_writes() {
        let value = AnyJson::Object(vec![
            ("s".into(), AnyJson::String("a/b\u{2028}é\u{1}".into())),
            ("d".into(), AnyJson::Double(0.1)),
            ("i".into(), AnyJson::Int(3)),
            ("b".into(), AnyJson::Bool(true)),
            ("arr".into(), AnyJson::Array(vec![])),
            ("obj".into(), AnyJson::Object(vec![])),
            ("n".into(), AnyJson::Null),
            ("big".into(), AnyJson::Double(1e16)),
            ("f".into(), AnyJson::Double(1.5)),
        ]);
        let options = WritingOptions { sorted_keys: true, ..WritingOptions::default() };
        assert_eq!(
            String::from_utf8(data(&value, options).unwrap()).unwrap(),
            "{\"arr\":[],\"b\":true,\"big\":10000000000000000,\"d\":0.10000000000000001,\"f\":1.5,\"i\":3,\"n\":null,\"obj\":{},\"s\":\"a\\/b\u{2028}é\\u0001\"}"
        );
    }

    #[test]
    fn sorted_keys_use_nsstring_order() {
        let keys = [
            "b", "B", "a", "A", "a10", "a2", "a_b", "aB", "Ab", "é", "e", "f", "z", "_x", "-y", "1", "10", "2", "ä",
            "ａ", "ab", "a b", "Z", "ß", "ss", "st",
        ];
        let value = AnyJson::Object(
            keys.iter().enumerate().map(|(index, key)| (key.to_string(), AnyJson::Int(index as i64))).collect(),
        );
        let options = WritingOptions { sorted_keys: true, ..WritingOptions::default() };
        assert_eq!(
            String::from_utf8(data(&value, options).unwrap()).unwrap(),
            r#"{"_x":13,"-y":14,"1":15,"2":17,"10":16,"a":2,"ａ":19,"A":3,"ä":18,"a b":21,"a_b":6,"a2":5,"a10":4,"ab":20,"aB":7,"Ab":8,"b":0,"B":1,"e":10,"é":9,"f":11,"ss":24,"ß":23,"st":25,"z":12,"Z":22}"#
        );
    }

    #[test]
    fn round_trips_numbers_as_foundation_read_them() {
        let text = br#"{"a":1,"b":1.0,"c":0.1,"d":true,"e":[null,"x"],"f":12345678901234567890}"#;
        let value = json_object(text, ReadingOptions::default()).unwrap();
        assert!(matches!(value.get("d"), Some(AnyJson::Bool(true))));
        let options = WritingOptions { sorted_keys: true, ..WritingOptions::default() };
        let written = String::from_utf8(data(&value, options).unwrap()).unwrap();
        assert_eq!(written, r#"{"a":1,"b":1,"c":0.10000000000000001,"d":true,"e":[null,"x"],"f":12345678901234567890}"#);
    }

    #[test]
    fn reports_malformed_input() {
        assert!(json_object(b"{", ReadingOptions::default()).is_err());
    }
}
