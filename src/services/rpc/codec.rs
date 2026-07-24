use std::{error::Error, fmt, str};

use serde::Serialize;

use super::protocol::{IncomingRecord, ProtocolDecodeError, decode_record};

/// Maximum JSON payload size for one inbound or outbound record: 64 MiB, excluding
/// the LF delimiter and an optional CR immediately before it.
///
/// A limit prevents a broken or hostile child from growing the transport buffer
/// without bound. Pi returns a complete session transcript as one record, so the
/// bound must also accommodate legitimately large tasks without disconnecting.
pub const DEFAULT_MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct JsonlCodec {
    buffer: Vec<u8>,
    max_frame_size: usize,
    next_frame: u64,
    state: DecoderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecoderState {
    Open,
    Faulted,
    Finished,
}

impl Default for JsonlCodec {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_SIZE)
    }
}

impl JsonlCodec {
    pub fn new(max_frame_size: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_frame_size,
            next_frame: 1,
            state: DecoderState::Open,
        }
    }

    pub fn max_frame_size(&self) -> usize {
        self.max_frame_size
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<IncomingRecord>, JsonlDecodeError> {
        match self.state {
            DecoderState::Faulted => return Err(JsonlDecodeError::DecoderFaulted),
            DecoderState::Finished => return Err(JsonlDecodeError::DecoderFinished),
            DecoderState::Open => {}
        }

        let mut records = Vec::new();
        let mut start = 0;
        for (index, byte) in chunk.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }

            let segment = &chunk[start..index];
            self.append_complete_segment(segment)?;
            records.push(self.decode_buffered_frame()?);
            start = index + 1;
        }
        self.append_incomplete_segment(&chunk[start..])?;
        Ok(records)
    }

    /// Decodes a final unterminated record, if present.
    ///
    /// Pi normally terminates every record with LF, while accepting the final
    /// unterminated record makes EOF handling deterministic and matches its
    /// installed reference JSONL reader. An empty buffer produces no record.
    pub fn finish(&mut self) -> Result<Option<IncomingRecord>, JsonlDecodeError> {
        match self.state {
            DecoderState::Faulted => return Err(JsonlDecodeError::DecoderFaulted),
            DecoderState::Finished => return Err(JsonlDecodeError::DecoderFinished),
            DecoderState::Open => {}
        }
        self.state = DecoderState::Finished;
        if self.buffer.is_empty() {
            return Ok(None);
        }

        let frame = self.next_frame;
        let mut bytes = std::mem::take(&mut self.buffer);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        self.decode_frame(frame, &bytes).map(Some)
    }

    pub fn encode<T: Serialize>(&self, record: &T) -> Result<Vec<u8>, JsonlEncodeError> {
        encode_record_with_limit(record, self.max_frame_size)
    }

    fn append_complete_segment(&mut self, segment: &[u8]) -> Result<(), JsonlDecodeError> {
        let projected = self.buffer.len().saturating_add(segment.len());
        let has_terminal_cr = segment
            .last()
            .copied()
            .or_else(|| self.buffer.last().copied())
            == Some(b'\r');
        let payload_size = projected.saturating_sub(usize::from(has_terminal_cr));
        if payload_size > self.max_frame_size {
            return self.fail(JsonlDecodeError::FrameTooLarge {
                frame: self.next_frame,
                size: payload_size,
                max: self.max_frame_size,
            });
        }
        self.buffer.extend_from_slice(segment);
        Ok(())
    }

    fn append_incomplete_segment(&mut self, segment: &[u8]) -> Result<(), JsonlDecodeError> {
        let projected = self.buffer.len().saturating_add(segment.len());
        let can_be_max_payload_plus_cr = projected == self.max_frame_size.saturating_add(1)
            && segment
                .last()
                .copied()
                .or_else(|| self.buffer.last().copied())
                == Some(b'\r');
        if projected > self.max_frame_size && !can_be_max_payload_plus_cr {
            return self.fail(JsonlDecodeError::FrameTooLarge {
                frame: self.next_frame,
                size: projected,
                max: self.max_frame_size,
            });
        }
        self.buffer.extend_from_slice(segment);
        Ok(())
    }

    fn decode_buffered_frame(&mut self) -> Result<IncomingRecord, JsonlDecodeError> {
        let frame = self.next_frame;
        self.next_frame = self.next_frame.saturating_add(1);
        let mut bytes = std::mem::take(&mut self.buffer);
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        self.decode_frame(frame, &bytes)
    }

    fn decode_frame(
        &mut self,
        frame: u64,
        bytes: &[u8],
    ) -> Result<IncomingRecord, JsonlDecodeError> {
        if bytes.is_empty() {
            return self.fail(JsonlDecodeError::BlankFrame { frame });
        }
        if let Err(error) = str::from_utf8(bytes) {
            return self.fail(JsonlDecodeError::InvalidUtf8 {
                frame,
                valid_up_to: error.valid_up_to(),
                error_len: error.error_len(),
            });
        }
        decode_record(bytes).map_err(|source| {
            self.state = DecoderState::Faulted;
            JsonlDecodeError::InvalidRecord { frame, source }
        })
    }

    fn fail<T>(&mut self, error: JsonlDecodeError) -> Result<T, JsonlDecodeError> {
        self.state = DecoderState::Faulted;
        Err(error)
    }
}

pub fn encode_record<T: Serialize>(record: &T) -> Result<Vec<u8>, JsonlEncodeError> {
    encode_record_with_limit(record, DEFAULT_MAX_FRAME_SIZE)
}

fn encode_record_with_limit<T: Serialize>(
    record: &T,
    max_frame_size: usize,
) -> Result<Vec<u8>, JsonlEncodeError> {
    let mut bytes = serde_json::to_vec(record).map_err(JsonlEncodeError::Serialization)?;
    if bytes.len() > max_frame_size {
        return Err(JsonlEncodeError::FrameTooLarge {
            size: bytes.len(),
            max: max_frame_size,
        });
    }
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Debug)]
pub enum JsonlDecodeError {
    DecoderFaulted,
    DecoderFinished,
    BlankFrame {
        frame: u64,
    },
    FrameTooLarge {
        frame: u64,
        size: usize,
        max: usize,
    },
    InvalidUtf8 {
        frame: u64,
        valid_up_to: usize,
        error_len: Option<usize>,
    },
    InvalidRecord {
        frame: u64,
        source: ProtocolDecodeError,
    },
}

impl fmt::Display for JsonlDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DecoderFaulted => {
                formatter.write_str("JSONL decoder is faulted; replace the connection")
            }
            Self::DecoderFinished => formatter.write_str("JSONL decoder already reached EOF"),
            Self::BlankFrame { frame } => write!(formatter, "RPC frame {frame} is blank"),
            Self::FrameTooLarge { frame, size, max } => write!(
                formatter,
                "RPC frame {frame} is {size} bytes, exceeding the {max}-byte limit"
            ),
            Self::InvalidUtf8 {
                frame,
                valid_up_to,
                error_len,
            } => write!(
                formatter,
                "RPC frame {frame} contains invalid UTF-8 at byte {valid_up_to} (invalid length {error_len:?})"
            ),
            Self::InvalidRecord { frame, source } => {
                write!(
                    formatter,
                    "RPC frame {frame} could not be decoded: {source}"
                )
            }
        }
    }
}

impl Error for JsonlDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRecord { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum JsonlEncodeError {
    Serialization(serde_json::Error),
    FrameTooLarge { size: usize, max: usize },
}

impl fmt::Display for JsonlEncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(error) => {
                write!(formatter, "failed to serialize RPC record: {error}")
            }
            Self::FrameTooLarge { size, max } => write!(
                formatter,
                "serialized RPC record is {size} bytes, exceeding the {max}-byte limit"
            ),
        }
    }
}

impl Error for JsonlEncodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialization(error) => Some(error),
            Self::FrameTooLarge { .. } => None,
        }
    }
}
