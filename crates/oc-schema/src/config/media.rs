//! `config/media.ts` — `Config.Media`.

use serde::{Deserialize, Serialize};

use crate::common::PositiveInt;

/// `Config.Media.Image`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Image {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<PositiveInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<PositiveInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_base64_bytes: Option<PositiveInt>,
}

/// `Config.Media`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Media {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Image>,
}
