//! Windows automation & application tools (Plan.txt §25-27).

use async_trait::async_trait;
use serde_json::json;

use crate::{
    check_permission, PermissionLevel, RiskLevel, Tool, ToolContext, ToolError, ToolInput,
    ToolManifest, ToolResult,
};

/// Characters that could be abused for command chaining or injection in cmd.exe.
const FORBIDDEN_SHELL_CHARS: &[char] = &['&', '|', ';', '<', '>', '^', '%', '"', '\'', '`', '\r', '\n'];

/// System shells, interpreters, and sensitive executables that cannot be launched via launch_application.
const FORBIDDEN_LAUNCH_TARGETS: &[&str] = &[
    "cmd", "cmd.exe",
    "powershell", "powershell.exe",
    "pwsh", "pwsh.exe",
    "bash", "bash.exe",
    "wsl", "wsl.exe",
    "mshta", "mshta.exe",
    "cscript", "cscript.exe",
    "wscript", "wscript.exe",
    "rundll32", "rundll32.exe",
    "regsvr32", "regsvr32.exe",
    "certutil", "certutil.exe",
    "reg", "reg.exe",
    "regedit", "regedit.exe",
    "vssadmin", "vssadmin.exe",
    "diskpart", "diskpart.exe",
    "bcdedit", "bcdedit.exe",
];

/// Executable script extensions prohibited in launch_application.
const FORBIDDEN_LAUNCH_EXTENSIONS: &[&str] = &[
    ".bat", ".cmd", ".ps1", ".vbs", ".vbe", ".js", ".jse", ".wsf", ".wsh", ".hta", ".scr", ".pif", ".reg",
];

/// Validates that an argument string does not contain cmd.exe meta/injection characters.
fn validate_safe_arg(name: &str, s: &str) -> Result<(), ToolError> {
    if s.is_empty() {
        return Err(ToolError::Failed("launch_application".into(), format!("{name} cannot be empty")));
    }
    if s.chars().any(|c| FORBIDDEN_SHELL_CHARS.contains(&c)) {
        return Err(ToolError::Failed(
            "launch_application".into(),
            format!("security violation: {name} contains forbidden shell characters: '{s}'"),
        ));
    }
    Ok(())
}

/// Validates that a launch target is not a system shell, dangerous script, or malicious URL scheme.
fn validate_safe_target(target: &str) -> Result<(), ToolError> {
    validate_safe_arg("target", target)?;

    let lower = target.to_lowercase();

    // Check if target is a URI scheme
    // Windows drive paths start with a single letter followed by ':' and slash/backslash
    let is_windows_drive = lower.len() >= 2
        && lower.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        && lower.chars().nth(1) == Some(':')
        && (lower.len() == 2 || lower.chars().nth(2) == Some('\\') || lower.chars().nth(2) == Some('/'));

    if let Some(colon_pos) = lower.find(':') {
        if !is_windows_drive {
            let scheme = &lower[..colon_pos];
            if scheme != "http" && scheme != "https" && scheme != "mailto" {
                return Err(ToolError::Failed(
                    "launch_application".into(),
                    format!("security violation: disallowed URI scheme in target '{target}' (only http, https, mailto allowed)"),
                ));
            }
            return Ok(());
        }
    }

    // Extract binary/filename without directory path
    let filename = std::path::Path::new(&lower)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&lower);

    if FORBIDDEN_LAUNCH_TARGETS.contains(&filename) {
        return Err(ToolError::Failed(
            "launch_application".into(),
            format!("security violation: launching system shells or interpreters ('{target}') is prohibited via launch_application; use execute_command if privileged"),
        ));
    }

    for ext in FORBIDDEN_LAUNCH_EXTENSIONS {
        if lower.ends_with(ext) {
            return Err(ToolError::Failed(
                "launch_application".into(),
                format!("security violation: launching executable script files with extension '{ext}' is prohibited via launch_application"),
            ));
        }
    }

    Ok(())
}

/// Masks sensitive keywords in window titles to prevent credential/privacy leakage.
fn mask_sensitive_window_title(title: &str) -> String {
    let lower = title.to_lowercase();
    let sensitive_keywords = [
        "password",
        "keepass",
        "1password",
        "bitwarden",
        "lastpass",
        "inprivate",
        "incognito",
        "private browsing",
        "authenticator",
        "secret",
        "credential",
    ];

    for kw in sensitive_keywords {
        if lower.contains(kw) {
            return "[Redacted - Sensitive Window Title]".to_string();
        }
    }
    title.to_string()
}

/// Tool for launching desktop applications, files, or web URLs on Windows.
pub struct LaunchAppTool;

#[async_trait]
impl Tool for LaunchAppTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "launch_application".into(),
            description: "Launches a Windows desktop application, file with its default program, or web URL.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Application name (e.g. 'notepad', 'calc'), file path, or URL"
                    },
                    "arguments": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command-line arguments"
                    }
                },
                "required": ["target"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string" },
                    "status": { "type": "string" }
                }
            }),
            permission: PermissionLevel::LowRisk,
            risk: RiskLevel::Low,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let target = input["target"]
            .as_str()
            .ok_or_else(|| ToolError::Failed("launch_application".into(), "missing 'target' parameter".into()))?;

        // Prevent command injection, privilege escalation to shells/interpreters, and dangerous URL schemes
        validate_safe_target(target)?;

        let mut safe_args = Vec::new();
        if let Some(args) = input["arguments"].as_array() {
            for arg in args {
                if let Some(s) = arg.as_str() {
                    validate_safe_arg("argument", s)?;
                    safe_args.push(s);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(["/C", "start", "", target]);

            for arg in safe_args {
                cmd.arg(arg);
            }

            cmd.spawn().map_err(|e| {
                ToolError::Failed("launch_application".into(), format!("failed to launch '{target}': {e}"))
            })?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = target;
            let _ = safe_args;
        }

        Ok(ToolResult {
            output: json!({
                "target": target,
                "status": "launched"
            }),
        })
    }
}

/// Dangerous characters forbidden in raw URLs to prevent command/argument injection.
const FORBIDDEN_URL_CHARS: &[char] = &[
    '"', '\'', '`', ' ', '\r', '\n', '\0', '<', '>', '^', '|', ';',
];

/// Encodes query parameters so search terms cannot inject raw shell metacharacters.
fn percent_encode_query(input: &str) -> String {
    let mut encoded = String::new();
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

/// Tool for opening a web browser with a URL or web service on Windows.
pub struct OpenBrowserTool;

#[async_trait]
impl Tool for OpenBrowserTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "open_browser".into(),
            description: "Opens a web browser with a specific URL or web service (e.g. YouTube, Google, GitHub, or any website).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL or website name to open. Examples: 'https://www.youtube.com', 'youtube', 'https://github.com', 'google'."
                    }
                },
                "required": ["url"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "status": { "type": "string" }
                }
            }),
            permission: PermissionLevel::LowRisk,
            risk: RiskLevel::Low,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let raw_url = input["url"]
            .as_str()
            .or_else(|| input["target"].as_str())
            .ok_or_else(|| ToolError::Failed("open_browser".into(), "missing 'url' parameter".into()))?
            .trim();

        if raw_url.is_empty() {
            return Err(ToolError::Failed("open_browser".into(), "url cannot be empty".into()));
        }

        let lower_raw = raw_url.to_lowercase();
        // Check if raw_url specifies a URI scheme (like javascript:, file:, data:, etc.)
        if let Some(colon) = lower_raw.find(':') {
            let scheme = &lower_raw[..colon];
            if scheme != "http" && scheme != "https" {
                return Err(ToolError::Failed(
                    "open_browser".into(),
                    format!("security violation: unsupported or dangerous URI scheme '{scheme}:'"),
                ));
            }
        }

        // Normalize well-known service names and URLs
        let normalized_url = match lower_raw.as_str() {
            "youtube" | "yt" => "https://www.youtube.com".to_string(),
            "google" => "https://www.google.com".to_string(),
            "github" => "https://www.github.com".to_string(),
            "x" | "twitter" => "https://x.com".to_string(),
            other => {
                if other.starts_with("http://") || other.starts_with("https://") {
                    raw_url.to_string()
                } else if other.contains('.') && !other.contains(' ') {
                    format!("https://{raw_url}")
                } else {
                    format!("https://www.google.com/search?q={}", percent_encode_query(raw_url))
                }
            }
        };

        // Safety validation: forbid shell injection characters, spaces, and allow only http/https
        if normalized_url.chars().any(|c| FORBIDDEN_URL_CHARS.contains(&c)) {
            return Err(ToolError::Failed(
                "open_browser".into(),
                format!("security violation: url contains forbidden characters: '{normalized_url}'"),
            ));
        }

        let lower = normalized_url.to_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return Err(ToolError::Failed(
                "open_browser".into(),
                format!("security violation: only http and https URLs are allowed: '{normalized_url}'"),
            ));
        }

        #[cfg(target_os = "windows")]
        {
            // Use rundll32 url.dll,FileProtocolHandler which calls ShellExecute directly
            // without invoking cmd.exe shell parsing, preventing command injection
            let mut cmd = tokio::process::Command::new("rundll32.exe");
            cmd.args(["url.dll,FileProtocolHandler", &normalized_url]);
            cmd.spawn().map_err(|e| {
                ToolError::Failed("open_browser".into(), format!("failed to open browser for '{normalized_url}': {e}"))
            })?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = &normalized_url;
        }

        Ok(ToolResult {
            output: json!({
                "url": normalized_url,
                "status": "opened"
            }),
        })
    }
}

/// Tool for querying information about active/running windows.
pub struct ActiveWindowTool;

#[async_trait]
impl Tool for ActiveWindowTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "active_window".into(),
            description: "Queries the currently active foreground window or running desktop windows.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "process_name": { "type": "string" }
                }
            }),
            permission: PermissionLevel::Read,
            risk: RiskLevel::None,
        }
    }

    async fn execute(&self, _input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        #[cfg(target_os = "windows")]
        {
            let ps_script = r#"
                $ps = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } | Select-Object -First 5 ProcessName, MainWindowTitle
                $ps | ConvertTo-Json -Compress
            "#;

            let ps_future = tokio::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
                .output();

            let output = tokio::time::timeout(std::time::Duration::from_secs(5), ps_future)
                .await
                .map_err(|_| ToolError::Failed("active_window".into(), "timeout inspecting windows via powershell".into()))?
                .map_err(|e| {
                    ToolError::Failed("active_window".into(), format!("failed to inspect windows: {e}"))
                })?;

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let mut parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or(json!({
                "raw": stdout
            }));

            // Redact sensitive window titles to prevent credential/privacy leakage
            if let Some(arr) = parsed.as_array_mut() {
                for item in arr.iter_mut() {
                    if let Some(title) = item.get("MainWindowTitle").and_then(|t| t.as_str()) {
                        let masked = mask_sensitive_window_title(title);
                        item["MainWindowTitle"] = json!(masked);
                    }
                }
            } else if let Some(title) = parsed.get("MainWindowTitle").and_then(|t| t.as_str()) {
                let masked = mask_sensitive_window_title(title);
                parsed["MainWindowTitle"] = json!(masked);
            }

            Ok(ToolResult {
                output: json!({
                    "windows": parsed
                }),
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(ToolResult {
                output: json!({
                    "title": "mock_window",
                    "process_name": "mock"
                }),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_have_expected_names() {
        let launch = LaunchAppTool;
        assert_eq!(launch.manifest().name, "launch_application");
        assert_eq!(launch.manifest().permission, PermissionLevel::LowRisk);

        let active = ActiveWindowTool;
        assert_eq!(active.manifest().name, "active_window");
        assert_eq!(active.manifest().permission, PermissionLevel::Read);
    }

    #[test]
    fn validates_forbidden_characters_in_target() {
        assert!(validate_safe_arg("target", "notepad").is_ok());
        assert!(validate_safe_arg("target", "https://example.com").is_ok());

        assert!(validate_safe_arg("target", "calc & echo evil").is_err());
        assert!(validate_safe_arg("target", "calc | del C:\\").is_err());
        assert!(validate_safe_arg("target", "calc; evil").is_err());
        assert!(validate_safe_arg("target", "calc%VAR%").is_err());
    }

    #[test]
    fn validates_safe_target_rules() {
        assert!(validate_safe_target("notepad").is_ok());
        assert!(validate_safe_target("https://google.com").is_ok());
        assert!(validate_safe_target("http://127.0.0.1:8080").is_ok());

        // Forbidden interpreters / shells
        assert!(validate_safe_target("powershell").is_err());
        assert!(validate_safe_target("powershell.exe").is_err());
        assert!(validate_safe_target("cmd").is_err());
        assert!(validate_safe_target("cmd.exe").is_err());
        assert!(validate_safe_target("mshta").is_err());
        assert!(validate_safe_target("cscript").is_err());
        assert!(validate_safe_target("regedit").is_err());

        // Forbidden script extensions
        assert!(validate_safe_target("script.bat").is_err());
        assert!(validate_safe_target("payload.ps1").is_err());
        assert!(validate_safe_target("evil.vbs").is_err());

        // Dangerous URL schemes
        assert!(validate_safe_target("javascript:alert(1)").is_err());
        assert!(validate_safe_target("file:///C:/secret").is_err());
    }

    #[test]
    fn masks_sensitive_window_titles() {
        assert_eq!(
            mask_sensitive_window_title("KeePass - Passwords.kdbx"),
            "[Redacted - Sensitive Window Title]"
        );
        assert_eq!(
            mask_sensitive_window_title("Google Chrome - InPrivate"),
            "[Redacted - Sensitive Window Title]"
        );
        assert_eq!(
            mask_sensitive_window_title("Visual Studio Code - main.rs"),
            "Visual Studio Code - main.rs"
        );
    }

    #[tokio::test]
    async fn windows_tools_respect_permission_gate() {
        let launch = LaunchAppTool;
        let read_ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::Read, // LaunchAppTool requires LowRisk
            root_path: None,
        };

        let res = launch.execute(json!({"target": "notepad"}), &read_ctx).await;
        assert!(matches!(res, Err(ToolError::PermissionDenied { .. })));
    }

    #[tokio::test]
    async fn open_browser_respects_permissions_and_validates_url() {
        let browser = OpenBrowserTool;
        let read_ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::Read, // requires LowRisk
            root_path: None,
        };

        let res = browser.execute(json!({"url": "https://www.youtube.com"}), &read_ctx).await;
        assert!(matches!(res, Err(ToolError::PermissionDenied { .. })));

        let low_risk_ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::LowRisk,
            root_path: None,
        };

        // Reject dangerous characters in URL
        let bad_res = browser.execute(json!({"url": "https://youtube.com\" && calc"}), &low_risk_ctx).await;
        assert!(bad_res.is_err());

        // Reject dangerous schemes
        let bad_scheme = browser.execute(json!({"url": "javascript:alert(1)"}), &low_risk_ctx).await;
        assert!(bad_scheme.is_err());

        // Search term with spaces is properly percent-encoded
        let search_res = browser.execute(json!({"url": "hello world & test"}), &low_risk_ctx).await.unwrap();
        assert_eq!(
            search_res.output["url"],
            "https://www.google.com/search?q=hello+world+%26+test"
        );
    }
}
