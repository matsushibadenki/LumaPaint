//! Provider requests and image bytes stay in Rust; WebViews only send instructions.
#[cfg(any(target_os = "macos", test))]
use base64::Engine;
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", test))]
use serde_json::json;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{Emitter, Manager};
const LIMIT: usize = 32 * 1024 * 1024;
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
static LIBRARY: Mutex<()> = Mutex::new(());
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Openai,
    Gemini,
}
impl Provider {
    #[cfg(target_os = "macos")]
    fn name(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Gemini => "gemini",
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prompt {
    pub name: String,
    pub camera: String,
    pub film: String,
    pub lens: String,
    pub lighting: String,
    pub aperture: String,
    pub shutter: String,
    pub iso: String,
    pub bokeh: String,
    pub style: String,
    #[serde(default)]
    pub color_tone: String,
    #[serde(default)]
    pub mood: String,
    #[serde(default)]
    pub texture: String,
    #[serde(default)]
    pub composition: String,
    #[serde(default)]
    pub viewpoint: String,
    #[serde(default)]
    pub framing: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub light_direction: String,
    pub text: String,
}
impl Prompt {
    fn compose(&self) -> Result<String, String> {
        let fields = [
            ("Camera", &self.camera),
            ("Film", &self.film),
            ("Lens", &self.lens),
            ("Lighting", &self.lighting),
            ("Aperture", &self.aperture),
            ("Shutter speed", &self.shutter),
            ("ISO", &self.iso),
            ("Bokeh", &self.bokeh),
            ("Style", &self.style),
            ("Color palette", &self.color_tone),
            ("Mood", &self.mood),
            ("Texture and finish", &self.texture),
            ("Composition", &self.composition),
            ("Viewpoint", &self.viewpoint),
            ("Framing", &self.framing),
            ("Setting", &self.environment),
            ("Light direction", &self.light_direction),
        ];
        if self.text.trim().is_empty()
            || self.text.len() > 16000
            || fields.iter().any(|(_, v)| v.len() > 512)
            || self.name.len() > 200
        {
            return Err("Enter a prompt (maximum 16000 bytes) / プロンプトを入力してください / 请输入提示词".into());
        }
        let mut result = self.text.trim().to_owned();
        for (label, value) in fields {
            if !value.trim().is_empty() {
                result.push_str(&format!("\n{label}: {}", value.trim()));
            }
        }
        Ok(result)
    }
}
#[tauri::command]
pub fn ai_bookmarks(
    app: tauri::AppHandle,
    action: String,
    prompt: Option<Prompt>,
    name: Option<String>,
) -> Result<Vec<Prompt>, String> {
    let _guard = LIBRARY.lock().map_err(|_| "Library unavailable")?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = dir.join("ai-prompts.json");
    let mut library: Vec<Prompt> = match std::fs::read(&path) {
        Ok(bytes) if bytes.len() <= 4 * 1024 * 1024 => {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid prompt library")?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        _ => return Err("Cannot read prompt library".into()),
    };
    if library.len() > 512
        || library
            .iter()
            .any(|p| p.compose().is_err() || p.name.trim().is_empty())
    {
        return Err("Invalid prompt library".into());
    }
    match action.as_str() {
        "list" => return Ok(library),
        "save" => {
            let p = prompt.ok_or("Missing prompt")?;
            p.compose()?;
            if p.name.trim().is_empty() {
                return Err("Name required".into());
            }
            library.retain(|v| v.name != p.name);
            library.push(p);
        }
        "delete" => library.retain(|p| Some(&p.name) != name.as_ref()),
        _ => return Err("Invalid library action".into()),
    }
    if library.len() > 512 {
        return Err("Prompt library is full".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec(&library).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    let _ = app.emit("ai-bookmarks-changed", ());
    Ok(library)
}
// Native Keychain calls avoid keys in application files, logs and process arguments.
#[cfg(target_os = "macos")]
mod secrets {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    const SERVICE: &str = "com.lumapaint.ai";
    pub fn access(
        provider: super::Provider,
        update: Option<&str>,
    ) -> Result<Option<String>, String> {
        if let Some(key) = update {
            let result = if key.is_empty() {
                delete_generic_password(SERVICE, provider.name())
            } else {
                set_generic_password(SERVICE, provider.name(), key.as_bytes())
            };
            if let Err(e) = result {
                if e.code() != -25300 {
                    return Err(
                        "Cannot update Keychain / キーチェーンを更新できません / 无法更新钥匙串"
                            .into(),
                    );
                }
            }
        }
        match get_generic_password(SERVICE, provider.name()) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "Invalid stored credential".into()),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(_) => Err(
                "Keychain access failed / キーチェーンにアクセスできません / 无法访问钥匙串".into(),
            ),
        }
    }
}
#[cfg(not(target_os = "macos"))]
mod secrets {
    pub fn access(
        _provider: super::Provider,
        _update: Option<&str>,
    ) -> Result<Option<String>, String> {
        Err("Secure credential storage is currently available on macOS / 安全なキー保存は現在macOSで利用できます / 安全密钥存储目前仅支持macOS".into())
    }
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Cannot initialize HTTPS".into())
}
async fn response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("AI API HTTP {}: check key, model, quota and permissions / キー・モデル・利用枠・権限を確認してください / 请检查密钥、模型、配额和权限",status.as_u16()));
    }
    if response.content_length().is_some_and(|n| n > LIMIT as u64) {
        return Err("AI response too large".into());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "AI response interrupted")?
    {
        if bytes.len() + chunk.len() > LIMIT {
            return Err("AI response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid AI response".into())
}
#[tauri::command]
pub async fn ai_credentials(
    provider: Provider,
    action: String,
    key: Option<String>,
) -> Result<bool, String> {
    if action == "save" {
        let key = key.ok_or("Missing API key")?;
        if key.len() > 512 || key.chars().any(char::is_control) {
            return Err("Invalid API key".into());
        }
        secrets::access(provider, Some(key.trim()))?;
    }
    if action == "delete" {
        secrets::access(provider, Some(""))?;
    }
    if !matches!(action.as_str(), "save" | "delete" | "status" | "verify") {
        return Err("Invalid credential action".into());
    }
    let Some(key) = secrets::access(provider, None)? else {
        return Ok(false);
    };
    if action == "verify" {
        let client = client()?;
        let request = match provider {
            Provider::Openai => client
                .get("https://api.openai.com/v1/models")
                .bearer_auth(key),
            Provider::Gemini => client
                .get("https://generativelanguage.googleapis.com/v1beta/models")
                .header("x-goog-api-key", key),
        };
        response(request.send().await.map_err(|_| {
            "Authentication request failed / 認証通信に失敗しました / 验证请求失败"
        })?)
        .await?;
    }
    Ok(true)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub provider: Provider,
    pub model: String,
    pub prompt: Prompt,
    pub edit: bool,
}
struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}
#[tauri::command]
pub fn ai_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}
#[tauri::command]
pub async fn ai_generate(window: tauri::WebviewWindow, request: Request) -> Result<(), String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("Generation already running / 生成処理中です / 正在生成".into());
    }
    let _guard = BusyGuard;
    CANCEL.store(false, Ordering::SeqCst);
    let prompt = request.prompt.compose()?;
    if request.model.is_empty()
        || request.model.len() > 100
        || !request
            .model
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
    {
        return Err("Invalid model".into());
    }
    let key=secrets::access(request.provider,None)?.ok_or("Set API key in Preferences / 環境設定でAPIキーを設定してください / 请在偏好设置中配置API密钥")?;
    #[cfg(target_os = "macos")]
    let (target, workspace) = crate::canvas::on_main(window.clone(), move || {
        crate::canvas::platform::ai_prepare(request.edit)
    })
    .await?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, request, prompt, key);
        return Err("Native image editing currently requires macOS".into());
    }
    #[cfg(target_os = "macos")]
    {
        let mut target = tauri::async_runtime::spawn_blocking(move || {
            crate::canvas::platform::ai_input(target, workspace)
        })
        .await
        .map_err(|_| "Image preparation failed")??;
        if CANCEL.load(Ordering::SeqCst) {
            return Err("Cancelled / キャンセルしました / 已取消".into());
        }
        let input = target.input.take();
        let client = client()?;
        let req = match request.provider {
            Provider::Openai => {
                if let Some(png) = input {
                    let part = reqwest::multipart::Part::bytes(png)
                        .file_name("selection.png")
                        .mime_str("image/png")
                        .map_err(|_| "Invalid image")?;
                    client
                        .post("https://api.openai.com/v1/images/edits")
                        .bearer_auth(key)
                        .multipart(
                            reqwest::multipart::Form::new()
                                .text("model", request.model)
                                .text("prompt", prompt)
                                .part("image[]", part),
                        )
                } else {
                    client.post("https://api.openai.com/v1/images/generations").bearer_auth(key).json(&json!({"model":request.model,"prompt":prompt,"n":1,"output_format":"png"}))
                }
            }
            Provider::Gemini => {
                let mut parts = vec![json!({"text":prompt})];
                if let Some(bytes) = input {
                    parts.push(json!({"inlineData":{"mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)}}));
                }
                client.post(format!("https://generativelanguage.googleapis.com/v1/models/{}:generateContent",request.model)).header("x-goog-api-key",key).json(&json!({"contents":[{"parts":parts}],"generationConfig":{"responseModalities":["TEXT","IMAGE"]}}))
            }
        };
        let result=response(req.send().await.map_err(|_|"AI request failed or timed out / AI通信が失敗またはタイムアウトしました / AI请求失败或超时")?).await?;
        if CANCEL.load(Ordering::SeqCst) {
            return Err("Cancelled / キャンセルしました / 已取消".into());
        }
        let data = extract_image(request.provider, &result)?;
        let name = if request.prompt.name.trim().is_empty() {
            "AI Image".to_owned()
        } else {
            request.prompt.name
        };
        let prepared = tauri::async_runtime::spawn_blocking(move || target.finish(&data))
            .await
            .map_err(|_| "Image processing failed")??;
        crate::canvas::on_main(window, move || {
            if CANCEL.load(Ordering::SeqCst) {
                return Err("Cancelled".into());
            }
            crate::canvas::platform::ai_apply(prepared, name)
        })
        .await
    }
}
#[cfg(any(target_os = "macos", test))]
fn extract_image(provider: Provider, value: &Value) -> Result<Vec<u8>, String> {
    let data=match provider {
        Provider::Openai=>value.pointer("/data/0/b64_json").and_then(Value::as_str),
        Provider::Gemini=>value.pointer("/candidates/0/content/parts").and_then(Value::as_array).and_then(|parts|parts.iter().find_map(|p|p.pointer("/inlineData/data").or_else(||p.pointer("/inline_data/data")).and_then(Value::as_str))),
    }.ok_or("No image returned; check prompt and model / 画像が返されませんでした。プロンプトとモデルを確認してください / 未返回图像，请检查提示词和模型")?;
    if data.len() > LIMIT {
        return Err("Image too large".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| "Invalid image encoding".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_image_responses() {
        for (p, v) in [
            (Provider::Openai, json!({"data":[{"b64_json":"AQID"}]})),
            (
                Provider::Gemini,
                json!({"candidates":[{"content":{"parts":[{"text":"ok"},{"inlineData":{"data":"AQID"}}]}}]}),
            ),
        ] {
            assert_eq!(extract_image(p, &v).unwrap(), vec![1, 2, 3]);
        }
        assert!(extract_image(Provider::Openai, &json!({"data":[]})).is_err());
    }
    #[test]
    fn bounded_http_and_redacted_errors() {
        use std::io::{Read, Write};
        for (status, body, length) in [
            (200, r#"{"data":[]}"#, None),
            (401, "secret-api-key", None),
            (200, "{}", Some(LIMIT + 1)),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let handle = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0; 4096];
                let _ = stream.read(&mut buffer);
                let reply=format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",length.unwrap_or(body.len()));
                stream.write_all(reply.as_bytes()).unwrap();
            });
            let result = tauri::async_runtime::block_on(async {
                response(client().unwrap().get(url).send().await.unwrap()).await
            });
            handle.join().unwrap();
            if status == 200 && length.is_none() {
                assert!(result.is_ok());
            } else {
                let error = result.unwrap_err();
                assert!(!error.contains("secret-api-key"));
                assert!(error.contains(if status == 401 { "401" } else { "too large" }));
            }
        }
    }
    #[test]
    fn detailed_prompt_preserves_old_bookmarks_and_round_trips() {
        let old = serde_json::json!({"name":"Legacy", "camera":"35mm", "film":"", "lens":"", "lighting":"", "aperture":"", "shutter":"", "iso":"", "bokeh":"", "style":"漫画風", "text":"A quiet street"});
        let mut prompt: Prompt = serde_json::from_value(old).unwrap();
        assert!(prompt.composition.is_empty());
        assert_eq!(prompt.style, "漫画風");
        prompt.composition = "Rule of thirds".into();
        prompt.light_direction = "Side light".into();
        prompt.color_tone = "Warm amber tones".into();
        let loaded: Prompt = serde_json::from_slice(&serde_json::to_vec(&prompt).unwrap()).unwrap();
        let composed = loaded.compose().unwrap();
        assert!(composed.contains("Composition: Rule of thirds"));
        assert!(composed.contains("Light direction: Side light"));
        assert!(composed.contains("Color palette: Warm amber tones"));
        assert!(composed.contains("Style: 漫画風"));
        prompt.mood = "a".repeat(513);
        assert!(prompt.compose().is_err());
    }
    #[test]
    fn structured_prompt() {
        let p = Prompt {
            text: "A cat".into(),
            camera: "35mm".into(),
            ..Default::default()
        };
        assert_eq!(p.compose().unwrap(), "A cat\nCamera: 35mm");
        assert!(Prompt::default().compose().is_err());
    }
}
