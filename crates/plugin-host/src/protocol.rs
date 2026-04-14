use serde::{Deserialize, Serialize};
use serde_json::Value;

const JSON_RPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

/// JSON-RPC 2.0 response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: Option<Value>, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.into(),
            id,
            method: method.into(),
            params,
        }
    }

    pub fn notification(method: &str, params: Value) -> Self {
        Self::new(None, method, params)
    }

    pub fn jsonrpc(&self) -> &str {
        &self.jsonrpc
    }

    pub fn id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn params(&self) -> &Value {
        &self.params
    }

    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError::new(code, message, None)),
        }
    }

    pub fn jsonrpc(&self) -> &str {
        &self.jsonrpc
    }

    pub fn id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    pub fn result(&self) -> Option<&Value> {
        self.result.as_ref()
    }

    pub fn error_object(&self) -> Option<&JsonRpcError> {
        self.error.as_ref()
    }
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    pub fn code(&self) -> i32 {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_notification_uses_json_rpc_defaults() {
        let request = JsonRpcRequest::notification("event", json!({"ok": true}));

        assert_eq!(request.jsonrpc(), "2.0");
        assert_eq!(request.method(), "event");
        assert_eq!(request.params(), &json!({"ok": true}));
        assert!(request.id().is_none());
        assert!(request.is_notification());
    }

    #[test]
    fn request_deserializes_missing_params_as_null() {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "execute"
        }))
        .expect("request should deserialize");

        assert_eq!(request.id(), Some(&json!(7)));
        assert_eq!(request.params(), &Value::Null);
    }

    #[test]
    fn response_error_exposes_error_payload() {
        let response = JsonRpcResponse::error(Some(json!(1)), -32603, "boom");

        assert_eq!(response.jsonrpc(), "2.0");
        assert_eq!(response.id(), Some(&json!(1)));
        assert!(response.result().is_none());
        let error = response.error_object().expect("error object");
        assert_eq!(error.code(), -32603);
        assert_eq!(error.message(), "boom");
        assert!(error.data().is_none());
    }
}
