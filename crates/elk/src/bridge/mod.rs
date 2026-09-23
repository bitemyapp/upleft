//! Port of elk-swift's `Bridge/` folder: the public API and the conversion
//! between JSON dictionaries and ELK's graph model.

pub mod elk;
pub mod elk_graph_impl;
pub mod elk_graph_util;
pub mod java_compat;
pub mod java_numbers;
pub mod json_exporter;
pub mod json_importer;
pub mod type_aliases;
