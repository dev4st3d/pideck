//! UI-independent compatibility boundary for Pi 0.80.10 RPC.

mod client;
mod codec;
mod protocol;
mod runtime_adapter;

pub use client::*;
pub use codec::{
    DEFAULT_MAX_FRAME_SIZE, JsonlCodec, JsonlDecodeError, JsonlEncodeError, encode_record,
};
pub use protocol::*;
pub use runtime_adapter::*;
