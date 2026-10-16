//! Raw FFI for the vendored miniaudio 0.11.22 (see `../README.md`).
//!
//! Bindings are pre-generated (`src/bindings.rs`, bindgen 0.70 — see
//! README.md for the regeneration recipe) and cover exactly the surface the
//! TUI sound engine needs: the no-device `ma_engine` mixer, the playback
//! `ma_device` pair, `ma_decoder`/`ma_data_source` decoding, and the
//! `ma_audio_buffer_ref` → `ma_sound` playback path v2's native engine uses.

#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
#![allow(clippy::missing_safety_doc, clippy::upper_case_acronyms)]

mod bindings;

pub use bindings::*;
