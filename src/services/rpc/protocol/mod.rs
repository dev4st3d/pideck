mod command;
mod common;
mod event;
mod extension;
mod ids;
mod response;
mod session;

use std::{error::Error, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

pub use command::*;
pub use common::*;
pub use event::*;
pub use extension::*;
pub use ids::*;
pub use response::*;
pub use session::*;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OutboundRecord {
    Command(RpcCommand),
    ExtensionUiResponse(ExtensionUiResponse),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IncomingRecord {
    Response(Box<RpcResponse>),
    Event(Box<RpcEvent>),
    ExtensionUiRequest(ExtensionUiRequest),
    ExtensionError(ExtensionError),
    UnknownEvent(UnknownEvent),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnknownEvent {
    pub event_type: String,
    pub raw: Value,
}

#[derive(Debug)]
pub enum ProtocolDecodeError {
    InvalidJson(serde_json::Error),
    TopLevelNotObject,
    MissingType,
    TypeNotString,
    InvalidRecord {
        record_type: String,
        message: String,
    },
}

impl fmt::Display for ProtocolDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(formatter, "invalid JSON record: {error}"),
            Self::TopLevelNotObject => formatter.write_str("RPC record must be a JSON object"),
            Self::MissingType => formatter.write_str("RPC record requires a `type` field"),
            Self::TypeNotString => formatter.write_str("RPC record `type` must be a string"),
            Self::InvalidRecord {
                record_type,
                message,
            } => write!(formatter, "invalid `{record_type}` RPC record: {message}"),
        }
    }
}

impl Error for ProtocolDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
            _ => None,
        }
    }
}

pub fn decode_record(bytes: &[u8]) -> Result<IncomingRecord, ProtocolDecodeError> {
    let value = serde_json::from_slice(bytes).map_err(ProtocolDecodeError::InvalidJson)?;
    IncomingRecord::from_value(value)
}

impl IncomingRecord {
    pub fn from_value(value: Value) -> Result<Self, ProtocolDecodeError> {
        let object = value
            .as_object()
            .ok_or(ProtocolDecodeError::TopLevelNotObject)?;
        let record_type = object
            .get("type")
            .ok_or(ProtocolDecodeError::MissingType)?
            .as_str()
            .ok_or(ProtocolDecodeError::TypeNotString)?
            .to_owned();

        fn invalid(record_type: &str, error: impl fmt::Display) -> ProtocolDecodeError {
            ProtocolDecodeError::InvalidRecord {
                record_type: record_type.to_owned(),
                message: error.to_string(),
            }
        }

        match record_type.as_str() {
            "response" => RpcResponse::from_value(value)
                .map(Box::new)
                .map(Self::Response)
                .map_err(|error| invalid(&record_type, error)),
            "extension_ui_request" => serde_json::from_value(value)
                .map(Self::ExtensionUiRequest)
                .map_err(|error| invalid(&record_type, error)),
            "extension_error" => serde_json::from_value(value)
                .map(Self::ExtensionError)
                .map_err(|error| invalid(&record_type, error)),
            known if is_known_event(known) => serde_json::from_value(value)
                .map(Box::new)
                .map(Self::Event)
                .map_err(|error| invalid(&record_type, error)),
            _ => Ok(Self::UnknownEvent(UnknownEvent {
                event_type: record_type,
                raw: value,
            })),
        }
    }
}

impl Serialize for IncomingRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Response(response) => response.serialize(serializer),
            Self::Event(event) => event.serialize(serializer),
            Self::ExtensionUiRequest(request) => request.serialize(serializer),
            Self::UnknownEvent(event) => event.raw.serialize(serializer),
            Self::ExtensionError(error) => {
                let mut value = serde_json::to_value(error).map_err(serde::ser::Error::custom)?;
                value
                    .as_object_mut()
                    .ok_or_else(|| {
                        serde::ser::Error::custom("extension error must serialize as an object")
                    })?
                    .insert(
                        "type".to_owned(),
                        Value::String("extension_error".to_owned()),
                    );
                value.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for IncomingRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(value).map_err(D::Error::custom)
    }
}
