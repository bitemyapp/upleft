//! Raw bindings to cmark-gfm, compiled from `vendor/swift-cmark` at the
//! revision Downright's `Package.resolved` pins. Only the surface
//! swift-markdown's `CommonMarkConverter` touches is declared here; the
//! parse must be byte-for-byte the one Downright gets, so this crate never
//! patches or reimplements the C library.
#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_uint, c_void};

#[repr(C)]
pub struct cmark_node {
    _private: [u8; 0],
}

#[repr(C)]
pub struct cmark_parser {
    _private: [u8; 0],
}

#[repr(C)]
pub struct cmark_iter {
    _private: [u8; 0],
}

#[repr(C)]
pub struct cmark_syntax_extension {
    _private: [u8; 0],
}

pub type cmark_node_type = c_uint;
pub type cmark_event_type = c_uint;
pub type cmark_list_type = c_uint;
pub type cmark_delim_type = c_uint;

pub const CMARK_NODE_TYPE_BLOCK: cmark_node_type = 0x8000;
pub const CMARK_NODE_TYPE_INLINE: cmark_node_type = 0x4000;

pub const CMARK_NODE_NONE: cmark_node_type = 0x0000;
pub const CMARK_NODE_DOCUMENT: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0001;
pub const CMARK_NODE_BLOCK_QUOTE: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0002;
pub const CMARK_NODE_LIST: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0003;
pub const CMARK_NODE_ITEM: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0004;
pub const CMARK_NODE_CODE_BLOCK: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0005;
pub const CMARK_NODE_HTML_BLOCK: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0006;
pub const CMARK_NODE_CUSTOM_BLOCK: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0007;
pub const CMARK_NODE_PARAGRAPH: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0008;
pub const CMARK_NODE_HEADING: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x0009;
pub const CMARK_NODE_THEMATIC_BREAK: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x000a;
pub const CMARK_NODE_FOOTNOTE_DEFINITION: cmark_node_type = CMARK_NODE_TYPE_BLOCK | 0x000b;

pub const CMARK_NODE_TEXT: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0001;
pub const CMARK_NODE_SOFTBREAK: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0002;
pub const CMARK_NODE_LINEBREAK: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0003;
pub const CMARK_NODE_CODE: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0004;
pub const CMARK_NODE_HTML_INLINE: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0005;
pub const CMARK_NODE_CUSTOM_INLINE: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0006;
pub const CMARK_NODE_EMPH: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0007;
pub const CMARK_NODE_STRONG: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0008;
pub const CMARK_NODE_LINK: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x0009;
pub const CMARK_NODE_IMAGE: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x000a;
pub const CMARK_NODE_FOOTNOTE_REFERENCE: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x000b;
pub const CMARK_NODE_ATTRIBUTE: cmark_node_type = CMARK_NODE_TYPE_INLINE | 0x000c;

pub const CMARK_NO_LIST: cmark_list_type = 0;
pub const CMARK_BULLET_LIST: cmark_list_type = 1;
pub const CMARK_ORDERED_LIST: cmark_list_type = 2;

pub const CMARK_NO_DELIM: cmark_delim_type = 0;
pub const CMARK_PERIOD_DELIM: cmark_delim_type = 1;
pub const CMARK_PAREN_DELIM: cmark_delim_type = 2;

pub const CMARK_EVENT_NONE: cmark_event_type = 0;
pub const CMARK_EVENT_DONE: cmark_event_type = 1;
pub const CMARK_EVENT_ENTER: cmark_event_type = 2;
pub const CMARK_EVENT_EXIT: cmark_event_type = 3;

pub const CMARK_OPT_DEFAULT: c_int = 0;
pub const CMARK_OPT_SOURCEPOS: c_int = 1 << 1;
pub const CMARK_OPT_HARDBREAKS: c_int = 1 << 2;
pub const CMARK_OPT_UNSAFE: c_int = 1 << 17;
pub const CMARK_OPT_NOBREAKS: c_int = 1 << 4;
pub const CMARK_OPT_VALIDATE_UTF8: c_int = 1 << 9;
pub const CMARK_OPT_SMART: c_int = 1 << 10;
pub const CMARK_OPT_FOOTNOTES: c_int = 1 << 13;
pub const CMARK_OPT_TABLE_SPANS: c_int = 1 << 20;
pub const CMARK_OPT_TABLE_ROWSPAN_DITTO: c_int = 1 << 21;

unsafe extern "C" {
    pub fn cmark_parser_new(options: c_int) -> *mut cmark_parser;
    pub fn cmark_parser_free(parser: *mut cmark_parser);
    pub fn cmark_parser_feed(parser: *mut cmark_parser, buffer: *const c_char, len: usize);
    pub fn cmark_parser_finish(parser: *mut cmark_parser) -> *mut cmark_node;
    pub fn cmark_parser_attach_syntax_extension(
        parser: *mut cmark_parser,
        extension: *mut cmark_syntax_extension,
    ) -> c_int;
    pub fn cmark_find_syntax_extension(name: *const c_char) -> *mut cmark_syntax_extension;

    pub fn cmark_node_free(node: *mut cmark_node);
    pub fn cmark_node_next(node: *mut cmark_node) -> *mut cmark_node;
    pub fn cmark_node_previous(node: *mut cmark_node) -> *mut cmark_node;
    pub fn cmark_node_parent(node: *mut cmark_node) -> *mut cmark_node;
    pub fn cmark_node_first_child(node: *mut cmark_node) -> *mut cmark_node;
    pub fn cmark_node_last_child(node: *mut cmark_node) -> *mut cmark_node;
    pub fn cmark_node_get_user_data(node: *mut cmark_node) -> *mut c_void;
    pub fn cmark_node_get_type(node: *mut cmark_node) -> cmark_node_type;
    pub fn cmark_node_get_type_string(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_literal(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_heading_level(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_list_type(node: *mut cmark_node) -> cmark_list_type;
    pub fn cmark_node_get_list_delim(node: *mut cmark_node) -> cmark_delim_type;
    pub fn cmark_node_get_list_start(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_list_tight(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_item_index(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_fence_info(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_fenced(
        node: *mut cmark_node,
        length: *mut c_int,
        offset: *mut c_int,
        character: *mut c_char,
    ) -> c_int;
    pub fn cmark_node_get_url(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_title(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_on_enter(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_on_exit(node: *mut cmark_node) -> *const c_char;
    pub fn cmark_node_get_start_line(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_start_column(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_end_line(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_end_column(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_backtick_count(node: *mut cmark_node) -> c_int;
    pub fn cmark_node_get_attributes(node: *mut cmark_node) -> *const c_char;

    pub fn cmark_iter_new(root: *mut cmark_node) -> *mut cmark_iter;
    pub fn cmark_iter_free(iter: *mut cmark_iter);
    pub fn cmark_iter_next(iter: *mut cmark_iter) -> cmark_event_type;
    pub fn cmark_iter_get_node(iter: *mut cmark_iter) -> *mut cmark_node;
    pub fn cmark_iter_get_event_type(iter: *mut cmark_iter) -> cmark_event_type;
    pub fn cmark_iter_get_root(iter: *mut cmark_iter) -> *mut cmark_node;

    pub fn cmark_gfm_core_extensions_ensure_registered();
    pub fn cmark_gfm_extensions_get_table_columns(node: *mut cmark_node) -> u16;
    pub fn cmark_gfm_extensions_get_table_alignments(node: *mut cmark_node) -> *mut u8;
    pub fn cmark_gfm_extensions_get_table_row_is_header(node: *mut cmark_node) -> c_int;
    pub fn cmark_gfm_extensions_get_table_cell_colspan(node: *mut cmark_node) -> c_uint;
    pub fn cmark_gfm_extensions_get_table_cell_rowspan(node: *mut cmark_node) -> c_uint;
    pub fn cmark_gfm_extensions_get_tasklist_item_checked(node: *mut cmark_node) -> bool;

    pub fn cmark_markdown_to_html(text: *const c_char, len: usize, options: c_int) -> *mut c_char;
    pub fn cmark_version() -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    #[test]
    fn parses_a_table_with_the_extensions_swift_markdown_attaches() {
        unsafe {
            cmark_gfm_core_extensions_ensure_registered();
            let parser = cmark_parser_new(CMARK_OPT_TABLE_SPANS | CMARK_OPT_SOURCEPOS);
            for name in ["table", "strikethrough", "tasklist"] {
                let name = CString::new(name).unwrap();
                let extension = cmark_find_syntax_extension(name.as_ptr());
                assert!(!extension.is_null());
                assert_eq!(cmark_parser_attach_syntax_extension(parser, extension), 1);
            }
            let text = "| a | b |\n|---|:-:|\n| 1 | 2 |\n\n- [x] done ~~no~~\n";
            cmark_parser_feed(parser, text.as_ptr().cast(), text.len());
            let document = cmark_parser_finish(parser);
            let table = cmark_node_first_child(document);
            let kind = CStr::from_ptr(cmark_node_get_type_string(table));
            assert_eq!(kind.to_str().unwrap(), "table");
            assert_eq!(cmark_gfm_extensions_get_table_columns(table), 2);
            assert_eq!(cmark_node_get_start_line(table), 1);
            let list = cmark_node_next(table);
            assert_eq!(cmark_node_get_type(list), CMARK_NODE_LIST);
            let item = cmark_node_first_child(list);
            assert!(cmark_gfm_extensions_get_tasklist_item_checked(item));
            cmark_node_free(document);
            cmark_parser_free(parser);
        }
    }
}
