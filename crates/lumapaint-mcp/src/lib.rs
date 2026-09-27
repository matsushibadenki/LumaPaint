//! Initial local MCP host. Owns an isolated, ephemeral Rust document, not a GUI copy.
use lumapaint_core::document::{Document, NewDocumentSettings};
use serde_json::{json, Value};

#[derive(Default)]
pub struct Server {
    initialized: bool,
    ready: bool,
    document: Option<Document>,
    creation: Option<(Value, Value)>,
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn tool(name: &str, description: &str, schema: Value, readonly: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":schema,"annotations":{"readOnlyHint":readonly,"destructiveHint":false,"openWorldHint":false}})
}
fn empty_schema() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}
impl Server {
    pub fn handle(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        if message.get("jsonrpc") != Some(&json!("2.0"))
            || !message["method"].is_string()
            || id
                .as_ref()
                .is_some_and(|v| !v.is_string() && !v.is_number())
        {
            return Some(error(Value::Null, -32600, "Invalid request"));
        }
        let method = message["method"].as_str().unwrap();
        if id.is_none() {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        }
        let id = id.unwrap();
        let params = message.get("params").cloned().unwrap_or(json!({}));
        if !params.is_object() {
            return Some(error(id, -32602, "Parameters must be an object"));
        }
        let result = match method {
            "initialize" if !self.initialized => {
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Some(error(id, -32602, "Invalid initialize parameters"));
                }
                self.initialized = true;
                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"lumapaint-mcp","version":env!("CARGO_PKG_VERSION")},"instructions":"Isolated ephemeral engine host. No GUI connection, file access, rendering or AI generation yet. Document is lost when disconnected."})
            }
            "ping" => json!({}),
            _ if !self.ready => {
                return Some(error(
                    id,
                    -32000,
                    "Initialize and send notifications/initialized first",
                ))
            }
            "tools/list" => json!({"tools":[
                tool("lumapaint.capabilities","Inspect implemented capabilities and session limits",empty_schema(),true),
                tool("engine.document.get","Get the current ephemeral document snapshot",empty_schema(),true),
                tool("engine.document.create","Create one blank RGB document in this ephemeral session; reuse identical arguments to retry",json!({"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":256},"width":{"type":"integer","minimum":1,"maximum":8192},"height":{"type":"integer","minimum":1,"maximum":8192}},"required":["name","width","height"],"additionalProperties":false}),false)
            ]}),
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Some(error(id, -32602, "Missing tool name"));
                };
                if ![
                    "lumapaint.capabilities",
                    "engine.document.get",
                    "engine.document.create",
                ]
                .contains(&name)
                {
                    return Some(error(id, -32602, "Unknown tool"));
                }
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, args) {
                    Ok(data) => {
                        json!({"content":[{"type":"text","text":data.to_string()}],"structuredContent":data,"isError":false})
                    }
                    Err(message) => {
                        json!({"content":[{"type":"text","text":message}],"isError":true})
                    }
                }
            }
            _ => return Some(error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let object = args.as_object().ok_or("Arguments must be an object")?;
        if name != "engine.document.create" && !object.is_empty() {
            return Err("This tool accepts no arguments".into());
        }
        match name {
            "lumapaint.capabilities" => Ok(
                json!({"host":"isolated-ephemeral","engine":{"createDocument":true,"documentSnapshot":true,"render":false,"edit":false,"export":false,"generation":false},"gui":{"connected":false},"limits":{"documents":1,"maxMessageBytes":65536},"runtime":lumapaint_core::runtime_info()}),
            ),
            "engine.document.get" => self
                .document
                .as_ref()
                .map(|d| json!({"documentId":"document-1","document":d.snapshot()}))
                .ok_or("No document in this session".into()),
            "engine.document.create" => {
                if let Some((previous, result)) = &self.creation {
                    return if previous == &args {
                        Ok(result.clone())
                    } else {
                        Err(
                            "A document already exists; start a new session to create another"
                                .into(),
                        )
                    };
                }
                if object.len() != 3
                    || !object.contains_key("name")
                    || !object.contains_key("width")
                    || !object.contains_key("height")
                {
                    return Err("Expected name, width and height only".into());
                }
                let name = args["name"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && s.chars().count() <= 256)
                    .ok_or("Invalid document name")?;
                for key in ["width", "height"] {
                    if !args[key].as_u64().is_some_and(|v| (1..=8192).contains(&v)) {
                        return Err(format!("Invalid {key}"));
                    }
                }
                let settings: NewDocumentSettings = serde_json::from_value(json!({"document":{"name":name,"width":args["width"],"height":args["height"],"unit":"pixels","resolution":72,"artboards":false,"canvasColor":"white","pixelAspectRatio":1.0},"colorMode":"rgb","colorProfile":"srgb","bitDepth":8})).map_err(|e|e.to_string())?;
                let document = Document::from_preset(settings)?;
                let result = json!({"documentId":"document-1","document":document.snapshot()});
                self.document = Some(document);
                self.creation = Some((args, result.clone()));
                Ok(result)
            }
            _ => Err("Unknown tool".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handshake_and_notifications() {
        let mut s = Server::default();
        assert!(s
            .handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .unwrap()
            .get("error")
            .is_some());
        assert!(s.handle(json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).unwrap().get("result").is_some());
        assert!(s
            .handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none());
        assert_eq!(
            s.handle(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}))
                .unwrap()["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
    #[test]
    fn creation_validates_retries_and_preserves_document() {
        let mut s = Server::default();
        assert!(s.call("engine.document.get", json!({})).is_err());
        assert!(s
            .call(
                "engine.document.create",
                json!({"name":"test","width":0,"height":20})
            )
            .is_err());
        let args = json!({"name":"日本語 中文 Test","width":400,"height":300});
        let result = s.call("engine.document.create", args.clone()).unwrap();
        assert_eq!(result["document"]["width"], 400);
        assert_eq!(s.call("engine.document.create", args).unwrap(), result);
        assert!(s
            .call(
                "engine.document.create",
                json!({"name":"other","width":200,"height":300})
            )
            .is_err());
        assert_eq!(s.call("engine.document.get", json!({})).unwrap(), result);
    }
}
