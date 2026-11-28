mod emoji_table;
#[allow(dead_code)]
mod emoji;
mod mime_tables;
mod country_table;
mod api_functions;
mod client_options;
mod tl_schema;

mod entity_scan;
mod json;
mod text_search;
mod text_entities;
mod text_parse;
mod sync_actions;
mod client_abi;

#[allow(unused_imports)]
pub(crate) use entity_scan::*;
#[allow(unused_imports)]
pub(crate) use json::*;
#[allow(unused_imports)]
pub(crate) use text_search::*;
#[allow(unused_imports)]
pub(crate) use text_entities::*;
#[allow(unused_imports)]
pub(crate) use text_parse::*;
#[allow(unused_imports)]
pub(crate) use sync_actions::*;
#[allow(unused_imports)]
pub(crate) use client_abi::*;

#[allow(dead_code)]
mod unicode_tables;

pub(crate) use unicode_tables::{
    PREPARE_SEARCH_CHARACTER_RANGES, PREPARE_SEARCH_CHARACTER_TABLE, TABLE_SIZE,
    UNICODE_SIMPLE_CATEGORY_RANGES, WITHOUT_DIACRITICS_RANGES, WITHOUT_DIACRITICS_TABLE,
};