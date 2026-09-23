//! The pure helpers of `Sources/DownrightQL/PreviewViewController.swift`.
//! The controller itself (an `NSViewController` hosting the renderer) is
//! ported with the UI layer.

use upleft_swift_text as swift_text;

/// `PreviewViewController`'s static members.
pub struct PreviewViewController;

impl PreviewViewController {
    /// Keep the rendered prefix bounded in both Foundation's UTF-16
    /// coordinate space and its UTF-8 storage size. Walking character
    /// boundaries avoids returning a string with a split surrogate when a
    /// limit lands mid-scalar.
    ///
    /// `boundedPrefix(_:utf16Limit:byteLimit:)`: steps by `Character`
    /// (`text.index(after:)`), so a grapheme cluster is kept or dropped whole.
    pub fn bounded_prefix(text: &str, utf16_limit: isize, byte_limit: isize) -> String {
        if !(utf16_limit > 0 && byte_limit > 0) {
            return String::new();
        }
        let mut utf16_count: isize = 0;
        let mut byte_count: isize = 0;
        let mut end = 0;
        for character in swift_text::graphemes(text) {
            let character_utf16_count = character.encode_utf16().count() as isize;
            let character_byte_count = character.len() as isize;
            if !(utf16_count + character_utf16_count <= utf16_limit && byte_count + character_byte_count <= byte_limit) {
                break;
            }
            utf16_count += character_utf16_count;
            byte_count += character_byte_count;
            end += character.len();
        }
        text[..end].to_owned()
    }

    /// Bytes in use across malloc zones. `malloc_zone_statistics` is what
    /// §10 specifies; it is cheap enough to poll and, unlike `task_info`,
    /// reports what this process actually allocated rather than what the
    /// kernel has mapped for it.
    pub fn resident_bytes() -> isize {
        let mut zone_count: libc::c_uint = 0;
        let mut zones: *mut libc::vm_address_t = std::ptr::null_mut();
        let result = unsafe {
            malloc_get_all_zones(mach_task_self_, std::ptr::null(), &mut zones, &mut zone_count)
        };
        if result != libc::KERN_SUCCESS || zones.is_null() {
            return 0;
        }
        let mut total: isize = 0;
        for index in 0..zone_count as usize {
            let zone = unsafe { *zones.add(index) } as *mut libc::c_void;
            if zone.is_null() {
                continue;
            }
            let mut statistics = MallocStatistics::default();
            unsafe { malloc_zone_statistics(zone, &mut statistics) };
            total += statistics.size_in_use as isize;
        }
        total
    }

    /// `previewMemoryBytes(current:baseline:)`: the preview's incremental
    /// footprint, never negative.
    pub fn preview_memory_bytes(current: isize, baseline: isize) -> isize {
        (current - baseline).max(0)
    }
}

/// `malloc_statistics_t`.
#[repr(C)]
#[derive(Default)]
struct MallocStatistics {
    blocks_in_use: libc::c_uint,
    size_in_use: libc::size_t,
    max_size_in_use: libc::size_t,
    size_allocated: libc::size_t,
}

unsafe extern "C" {
    static mach_task_self_: libc::mach_port_t;
    fn malloc_get_all_zones(
        task: libc::mach_port_t,
        reader: *const libc::c_void,
        addresses: *mut *mut libc::vm_address_t,
        count: *mut libc::c_uint,
    ) -> libc::kern_return_t;
    fn malloc_zone_statistics(zone: *mut libc::c_void, statistics: *mut MallocStatistics);
}
