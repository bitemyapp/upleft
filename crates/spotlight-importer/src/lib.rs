//! Port of `Sources/DownrightSpotlightImporter/SpotlightImporter.c`: the
//! classic MDImporter CFPlugIn ABI that mdworker expects. The Markdown parser
//! and metadata policy live in `upleft-spotlight-metadata`
//! (`DownrightSpotlightPopulateMetadata`); this crate owns only the ABI.
//!
//! Every function, the instance layout and the reference counting follow the
//! C line for line. The factory UUID and the importer's Info.plist are
//! Downright's (`Config/DownrightSpotlight-Info.plist`).

#![allow(non_snake_case)]

use std::ffi::c_void;

// Keep the metadata entry point linked into the bundle.
pub use upleft_spotlight_metadata::spotlight_metadata::DownrightSpotlightPopulateMetadata;

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFUUIDRef = *const c_void;
type CFStringRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type Boolean = u8;
type HRESULT = i32;
type ULONG = u32;
type LPVOID = *mut c_void;

/// `CFUUIDBytes`, which `REFIID` is passed by value as.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CFUUIDBytes {
    bytes: [u8; 16],
}

const S_OK: HRESULT = 0;
const E_NOINTERFACE: HRESULT = 0x8000_0004_u32 as i32;
const E_POINTER: HRESULT = 0x8000_0005_u32 as i32;
const E_OUTOFMEMORY: HRESULT = 0x8007_000E_u32 as i32;

const DOWNRIGHT_SPOTLIGHT_FACTORY_ID: &std::ffi::CStr = c"E55A6D2B-5C76-4E83-9C13-6F4BF1D98D77";

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFAllocatorSystemDefault: CFAllocatorRef;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> Boolean;
    fn CFUUIDCreateFromUUIDBytes(allocator: CFAllocatorRef, bytes: CFUUIDBytes) -> CFUUIDRef;
    fn CFUUIDCreateFromString(allocator: CFAllocatorRef, string: CFStringRef) -> CFUUIDRef;
    #[allow(clippy::too_many_arguments)]
    fn CFUUIDGetConstantUUIDWithBytes(
        allocator: CFAllocatorRef,
        b0: u8, b1: u8, b2: u8, b3: u8, b4: u8, b5: u8, b6: u8, b7: u8,
        b8: u8, b9: u8, b10: u8, b11: u8, b12: u8, b13: u8, b14: u8, b15: u8,
    ) -> CFUUIDRef;
    fn CFPlugInAddInstanceForFactory(factory_id: CFUUIDRef);
    fn CFPlugInRemoveInstanceForFactory(factory_id: CFUUIDRef);
    fn CFStringCreateWithCString(allocator: CFAllocatorRef, string: *const std::ffi::c_char, encoding: u32) -> CFStringRef;
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

/// `kMDImporterTypeID` (MDImporter.h).
unsafe fn md_importer_type_id() -> CFUUIDRef {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            kCFAllocatorDefault, 0x8B, 0x08, 0xC4, 0xBF, 0x41, 0x5B, 0x11, 0xD8, 0xB3, 0xF9, 0x00, 0x03, 0x93, 0x67,
            0x26, 0xFC,
        )
    }
}

/// `kMDImporterInterfaceID` (MDImporter.h).
unsafe fn md_importer_interface_id() -> CFUUIDRef {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            kCFAllocatorDefault, 0x6E, 0xBC, 0x27, 0xC4, 0x89, 0x9C, 0x11, 0xD8, 0x84, 0xAE, 0x00, 0x03, 0x93, 0x67,
            0x26, 0xFC,
        )
    }
}

/// `IUnknownUUID` (CFPlugInCOM.h).
unsafe fn iunknown_uuid() -> CFUUIDRef {
    unsafe {
        CFUUIDGetConstantUUIDWithBytes(
            kCFAllocatorSystemDefault, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x46,
        )
    }
}

/// `MDImporterInterfaceStruct`: `IUNKNOWN_C_GUTS` then `ImporterImportData`.
#[repr(C)]
pub struct MDImporterInterfaceStruct {
    _reserved: *mut c_void,
    query_interface: unsafe extern "C" fn(*mut c_void, CFUUIDBytes, *mut LPVOID) -> HRESULT,
    add_ref: unsafe extern "C" fn(*mut c_void) -> ULONG,
    release: unsafe extern "C" fn(*mut c_void) -> ULONG,
    importer_import_data: unsafe extern "C" fn(*mut c_void, CFMutableDictionaryRef, CFStringRef, CFStringRef) -> Boolean,
}

// SAFETY: a table of function pointers, never mutated.
unsafe impl Sync for MDImporterInterfaceStruct {}

/// `DownrightMetadataImporter`.
#[repr(C)]
struct DownrightMetadataImporter {
    interface_table: *const MDImporterInterfaceStruct,
    factory_id: CFUUIDRef,
    ref_count: u32,
}

static DOWNRIGHT_INTERFACE_TABLE: MDImporterInterfaceStruct = MDImporterInterfaceStruct {
    _reserved: std::ptr::null_mut(),
    query_interface: downright_query_interface,
    add_ref: downright_add_ref,
    release: downright_release,
    importer_import_data: downright_import_data,
};

unsafe extern "C" fn downright_import_data(
    _this_interface: *mut c_void,
    attributes: CFMutableDictionaryRef,
    content_type_uti: CFStringRef,
    path_to_file: CFStringRef,
) -> Boolean {
    // SAFETY: mdworker passes a mutable dictionary and two strings.
    let populated = unsafe { DownrightSpotlightPopulateMetadata(attributes, content_type_uti, path_to_file) };
    u8::from(populated)
}

unsafe fn downright_allocate(factory_id: CFUUIDRef) -> *mut DownrightMetadataImporter {
    // calloc, as the C does, so the instance is freed with free().
    let instance = unsafe { libc_calloc(1, std::mem::size_of::<DownrightMetadataImporter>()) } as *mut DownrightMetadataImporter;
    if instance.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        (*instance).interface_table = &DOWNRIGHT_INTERFACE_TABLE;
        (*instance).factory_id = CFRetain(factory_id);
        (*instance).ref_count = 1;
        CFPlugInAddInstanceForFactory(factory_id);
    }
    instance
}

unsafe fn downright_deallocate(instance: *mut DownrightMetadataImporter) {
    unsafe {
        CFPlugInRemoveInstanceForFactory((*instance).factory_id);
        CFRelease((*instance).factory_id);
        libc_free(instance.cast());
    }
}

unsafe extern "C" fn downright_query_interface(this_instance: *mut c_void, iid: CFUUIDBytes, ppv: *mut LPVOID) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
        let requested = CFUUIDCreateFromUUIDBytes(kCFAllocatorDefault, iid);
        if requested.is_null() {
            return E_OUTOFMEMORY;
        }
        let supported = CFEqual(requested, md_importer_interface_id()) != 0 || CFEqual(requested, iunknown_uuid()) != 0;
        CFRelease(requested);
        if !supported {
            return E_NOINTERFACE;
        }
        downright_add_ref(this_instance);
        *ppv = this_instance;
    }
    S_OK
}

unsafe extern "C" fn downright_add_ref(this_instance: *mut c_void) -> ULONG {
    let instance = this_instance as *mut DownrightMetadataImporter;
    unsafe {
        (*instance).ref_count += 1;
        (*instance).ref_count
    }
}

unsafe extern "C" fn downright_release(this_instance: *mut c_void) -> ULONG {
    let instance = this_instance as *mut DownrightMetadataImporter;
    unsafe {
        if (*instance).ref_count > 0 {
            (*instance).ref_count -= 1;
        }
        if (*instance).ref_count == 0 {
            downright_deallocate(instance);
            return 0;
        }
        (*instance).ref_count
    }
}

/// The factory named in the importer's Info.plist (`CFPlugInFactories`).
///
/// # Safety
/// Called by CFPlugIn with a valid type UUID.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn MetadataImporterPluginFactory(_allocator: CFAllocatorRef, type_id: CFUUIDRef) -> *mut c_void {
    unsafe {
        if CFEqual(type_id, md_importer_type_id()) == 0 {
            return std::ptr::null_mut();
        }
        let string = CFStringCreateWithCString(
            kCFAllocatorDefault,
            DOWNRIGHT_SPOTLIGHT_FACTORY_ID.as_ptr(),
            K_CF_STRING_ENCODING_UTF8,
        );
        let factory_id = CFUUIDCreateFromString(kCFAllocatorDefault, string);
        CFRelease(string);
        if factory_id.is_null() {
            return std::ptr::null_mut();
        }
        let instance = downright_allocate(factory_id);
        CFRelease(factory_id);
        instance.cast()
    }
}

unsafe extern "C" {
    #[link_name = "calloc"]
    fn libc_calloc(count: usize, size: usize) -> *mut c_void;
    #[link_name = "free"]
    fn libc_free(pointer: *mut c_void);
}
