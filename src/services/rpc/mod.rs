//! UI-independent compatibility boundary for Pi 0.80.10 RPC.

mod codec;
mod protocol;

pub use codec::{
    DEFAULT_MAX_FRAME_SIZE, JsonlCodec, JsonlDecodeError, JsonlEncodeError, encode_record,
};
pub use protocol::*;
