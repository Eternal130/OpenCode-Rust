//! Protocol utilities (port of `src/protocols/utils/`): incremental JSON
//! parsing, tool-call accumulation, lifecycle emission, Open Responses
//! options, hosted tool items, and the compaction checkpoint machines.

pub mod gemini_generate_content;
pub mod gemini_json_schema;
pub mod lifecycle;
pub mod open_responses_options;
pub mod partial_json;
pub mod responses_checkpoint;
pub mod responses_compaction;
pub mod responses_hosted_tools;
pub mod tool_stream;
