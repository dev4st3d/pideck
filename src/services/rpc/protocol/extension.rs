use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use super::RequestId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum KnownExtensionUiMethod {
    #[serde(rename = "select")]
    Select {
        title: String,
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "confirm")]
    Confirm {
        title: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "input")]
    Input {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "editor")]
    Editor {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prefill: Option<String>,
    },
    #[serde(rename = "notify")]
    Notify {
        message: String,
        #[serde(rename = "notifyType", skip_serializing_if = "Option::is_none")]
        notify_type: Option<NotificationType>,
    },
    #[serde(rename = "setStatus")]
    SetStatus {
        #[serde(rename = "statusKey")]
        status_key: String,
        #[serde(rename = "statusText", skip_serializing_if = "Option::is_none")]
        status_text: Option<String>,
    },
    #[serde(rename = "setWidget")]
    SetWidget {
        #[serde(rename = "widgetKey")]
        widget_key: String,
        #[serde(rename = "widgetLines", skip_serializing_if = "Option::is_none")]
        widget_lines: Option<Vec<String>>,
        #[serde(rename = "widgetPlacement", skip_serializing_if = "Option::is_none")]
        widget_placement: Option<WidgetPlacement>,
    },
    #[serde(rename = "setTitle")]
    SetTitle { title: String },
    #[serde(rename = "set_editor_text")]
    SetEditorText { text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtensionUiMethod {
    Known(KnownExtensionUiMethod),
    Unknown { method: String, raw: Value },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionUiRequest {
    pub id: RequestId,
    pub request: ExtensionUiMethod,
}

impl Serialize for ExtensionUiRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = match &self.request {
            ExtensionUiMethod::Known(request) => {
                serde_json::to_value(request).map_err(serde::ser::Error::custom)?
            }
            ExtensionUiMethod::Unknown { raw, .. } => raw.clone(),
        };
        let object = value
            .as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("extension UI request must be an object"))?;
        object.insert(
            "type".to_owned(),
            Value::String("extension_ui_request".to_owned()),
        );
        object.insert(
            "id".to_owned(),
            serde_json::to_value(&self.id).map_err(serde::ser::Error::custom)?,
        );
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExtensionUiRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("extension UI request must be an object"))?;
        if object.get("type").and_then(Value::as_str) != Some("extension_ui_request") {
            return Err(D::Error::custom(
                "extension UI request requires `type: \"extension_ui_request\"`",
            ));
        }
        let id = object
            .get("id")
            .cloned()
            .ok_or_else(|| D::Error::custom("extension UI request requires `id`"))?;
        let id = serde_json::from_value(id).map_err(D::Error::custom)?;
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::custom("extension UI request requires string `method`"))?;

        let request = match method {
            "select" | "confirm" | "input" | "editor" | "notify" | "setStatus" | "setWidget"
            | "setTitle" | "set_editor_text" => serde_json::from_value(value)
                .map(ExtensionUiMethod::Known)
                .map_err(D::Error::custom)?,
            unknown => ExtensionUiMethod::Unknown {
                method: unknown.to_owned(),
                raw: value,
            },
        };
        Ok(Self { id, request })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationType {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetPlacement {
    #[serde(rename = "aboveEditor")]
    AboveEditor,
    #[serde(rename = "belowEditor")]
    BelowEditor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionUiResponse {
    pub id: RequestId,
    pub response: ExtensionUiResponseBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtensionUiResponseBody {
    Value(String),
    Confirmed(bool),
    Cancelled,
}

impl Serialize for ExtensionUiResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Response<'a> {
            #[serde(rename = "type")]
            record_type: &'static str,
            id: &'a RequestId,
            #[serde(flatten)]
            body: ResponseBody<'a>,
        }

        #[derive(Serialize)]
        #[serde(untagged)]
        enum ResponseBody<'a> {
            Value { value: &'a str },
            Confirmed { confirmed: bool },
            Cancelled { cancelled: bool },
        }

        let body = match &self.response {
            ExtensionUiResponseBody::Value(value) => ResponseBody::Value { value },
            ExtensionUiResponseBody::Confirmed(confirmed) => ResponseBody::Confirmed {
                confirmed: *confirmed,
            },
            ExtensionUiResponseBody::Cancelled => ResponseBody::Cancelled { cancelled: true },
        };
        Response {
            record_type: "extension_ui_response",
            id: &self.id,
            body,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExtensionUiResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("extension UI response must be an object"))?;
        if object.get("type").and_then(Value::as_str) != Some("extension_ui_response") {
            return Err(D::Error::custom(
                "extension UI response requires `type: \"extension_ui_response\"`",
            ));
        }
        let id = object
            .get("id")
            .cloned()
            .ok_or_else(|| D::Error::custom("extension UI response requires `id`"))?;
        let id = serde_json::from_value(id).map_err(D::Error::custom)?;
        let response = if object.get("cancelled") == Some(&Value::Bool(true)) {
            ExtensionUiResponseBody::Cancelled
        } else if let Some(confirmed) = object.get("confirmed").and_then(Value::as_bool) {
            ExtensionUiResponseBody::Confirmed(confirmed)
        } else if let Some(value) = object.get("value").and_then(Value::as_str) {
            ExtensionUiResponseBody::Value(value.to_owned())
        } else {
            return Err(D::Error::custom(
                "extension UI response requires `value`, `confirmed`, or `cancelled: true`",
            ));
        };
        Ok(Self { id, response })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionError {
    #[serde(rename = "type")]
    pub record_type: ExtensionErrorRecordType,
    #[serde(rename = "extensionPath")]
    pub extension_path: String,
    pub event: String,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionErrorRecordType {
    #[serde(rename = "extension_error")]
    ExtensionError,
}
