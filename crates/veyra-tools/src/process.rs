//! Process execution tool (`execute_command`, Plan.txt section 25).
//!
//! Commands are permission-gated like every other tool. Output tails are
//! truncated so a chatty build cannot flood the event log / context.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    check_permission, PermissionLevel, RiskLevel, Tool, ToolContext, ToolError, ToolInput,
    ToolManifest, ToolResult,
};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const OUTPUT_TAIL: usize = 2000;

/// Run a shell command and capture its exit status and output.
pub struct CommandTool;

fn tail(s: &str) -> String {
    let s = s.trim_end();
    if s.len() <= OUTPUT_TAIL {
        s.to_string()
    } else {
        let target_cut = s.len() - OUTPUT_TAIL;
        // Find the first valid UTF-8 character boundary at or after target_cut
        let cut = s
            .char_indices()
            .map(|(i, _)| i)
            .find(|&i| i >= target_cut)
            .unwrap_or(s.len());
        let start = s[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
        s[start..].to_string()
    }
}

fn shell_parts(command: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        ("cmd".to_string(), vec!["/C".to_string(), command.to_string()])
    } else {
        ("sh".to_string(), vec!["-c".to_string(), command.to_string()])
    }
}

#[async_trait]
impl Tool for CommandTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "execute_command".into(),
            description: "Runs a shell command (e.g. build/test) and returns its status.".into(),
            input_schema: json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": {"type": "string"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "shell": {"type": "boolean"},
                    "cwd": {"type": "string"},
                    "timeout_secs": {"type": "integer"}
                }
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "success": {"type": "boolean"},
                    "exit_code": {"type": ["integer", "null"]},
                    "stdout": {"type": "string"},
                    "stderr": {"type": "string"},
                    "timed_out": {"type": "boolean"}
                }
            }),
            // General command execution allows arbitrary OS execution, so it is HighRisk.
            permission: PermissionLevel::HighRisk,
            risk: RiskLevel::High,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let command = input["command"].as_str().ok_or_else(|| {
            ToolError::Failed("execute_command".into(), "missing 'command'".into())
        })?;

        // Guard against catastrophic destructive commands.
        // Normalize whitespace and case for robust matching.
        let lower: String = command.to_lowercase();
        let normalized: String = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        let dangerous_patterns = [
            "rm -rf /",
            "rm -fr /",
            "rm -r -f /",
            "rm -f -r /",
            "rm -rf /*",
            "rm -rf ~",
            "rm -rf $home",
            "rmdir /s /q c:",
            "rmdir /q /s c:",
            "del /f /s /q c:",
            "del /s /q /f c:",
            "del /q /s /f c:",
            "del /f /q /s c:",
            "del /s /q c:",
            "del /q /s c:",
            "rd /s /q",
            "rd /q /s",
            "rmdir /q /s",
            "format c:",
            "format d:",
            "mkfs.",
            ":(){ :|:& };:",
            "dd if=/dev/zero",
            "dd if=/dev/random",
            "shutdown",
            "reboot",
            "> /dev/sda",
            "chmod -r 777 /",
            "chown -r",
            "reg delete hk",
            "vssadmin delete shadows",
            "bcdedit",
            "diskpart",
            "remove-item -recurse -force c:",
            "remove-item -force -recurse c:",
        ];
        for pattern in dangerous_patterns {
            if normalized.contains(pattern) {
                return Err(ToolError::Failed(
                    "execute_command".into(),
                    format!("blocked potentially catastrophic command pattern: '{pattern}'"),
                ));
            }
        }

        let shell = input["shell"].as_bool().unwrap_or(false);
        let timeout_secs = input["timeout_secs"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);

        let mut cmd = if shell {
            let (program, args) = shell_parts(command);
            let mut c = tokio::process::Command::new(program);
            c.args(args);
            c
        } else {
            let mut c = tokio::process::Command::new(command);
            if let Some(args) = input["args"].as_array() {
                for a in args {
                    if let Some(a) = a.as_str() {
                        c.arg(a);
                    }
                }
            }
            c
        };

        // Determine effective working directory:
        // - If cwd is provided, resolve relative paths against root_path if available.
        // - If cwd is omitted but root_path is available, default to root_path.
        let effective_cwd: Option<std::path::PathBuf> = if let Some(cwd) = input["cwd"].as_str() {
            let trimmed = cwd.trim();
            if trimmed.is_empty() {
                ctx.root_path.as_ref().map(|r| crate::fs::normalize_path(r))
            } else {
                let p = std::path::Path::new(trimmed);
                if let Some(ref root) = ctx.root_path {
                    let root_normalized = crate::fs::normalize_path(root);
                    let target = if p.is_absolute() {
                        crate::fs::normalize_path(p)
                    } else {
                        crate::fs::normalize_path(&root_normalized.join(p))
                    };
                    if !target.starts_with(&root_normalized) {
                        return Err(ToolError::Failed(
                            "execute_command".into(),
                            format!(
                                "access denied: cwd '{}' escapes sandbox root '{}'",
                                cwd, root_normalized.display()
                            ),
                        ));
                    }
                    Some(target)
                } else {
                    Some(crate::fs::normalize_path(p))
                }
            }
        } else {
            ctx.root_path.as_ref().map(|r| crate::fs::normalize_path(r))
        };

        if let Some(dir) = effective_cwd {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = cmd.spawn().map_err(|e| {
            ToolError::Failed("execute_command".into(), format!("failed to spawn: {e}"))
        })?;

        let wait = child.wait_with_output();
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), wait).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                Ok(ToolResult {
                    output: json!({
                        "success": output.status.success(),
                        "exit_code": output.status.code(),
                        "stdout": tail(&stdout),
                        "stderr": tail(&stderr),
                        "timed_out": false,
                    }),
                })
            }
            Ok(Err(e)) => Err(ToolError::Failed(
                "execute_command".into(),
                format!("failed to wait: {e}"),
            )),
            Err(_) => Ok(ToolResult {
                output: json!({
                    "success": false,
                    "exit_code": Value::Null,
                    "stdout": "",
                    "stderr": format!("timed out after {timeout_secs}s"),
                    "timed_out": true,
                }),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ToolContext {
        ToolContext {
            project_id: "p".into(),
            session_id: None,
            max_permission: PermissionLevel::HighRisk,
            root_path: None,
        }
    }

    #[tokio::test]
    async fn runs_command_and_reports_success() {
        let tool = CommandTool;
        let input = json!({"command": "exit 0", "shell": true});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert_eq!(result.output["success"], json!(true));
    }

    #[tokio::test]
    async fn reports_failure_on_nonzero_exit() {
        let tool = CommandTool;
        let input = json!({"command": "exit 3", "shell": true});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert_eq!(result.output["success"], json!(false));
        assert_eq!(result.output["exit_code"], json!(3));
    }

    #[tokio::test]
    async fn captures_stdout_tail() {
        let tool = CommandTool;
        let input = json!({"command": "echo veyra-check", "shell": true});
        let result = tool.execute(input, &ctx()).await.unwrap();
        assert_eq!(result.output["success"], json!(true));
        let stdout = result.output["stdout"].as_str().unwrap();
        assert!(stdout.contains("veyra-check"));
    }

    #[tokio::test]
    async fn rejects_overprivileged_context() {
        let tool = CommandTool;
        let restrictive = ToolContext {
            project_id: "p".into(),
            session_id: None,
            max_permission: PermissionLevel::LowRisk,
            root_path: None,
        };
        let input = json!({"command": "exit 0", "shell": true});
        let err = tool.execute(input, &restrictive).await.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn blocks_destructive_command() {
        let tool = CommandTool;
        let input = json!({"command": "rm -rf /", "shell": true});
        let err = tool.execute(input, &ctx()).await.unwrap_err();
        assert!(err.to_string().contains("blocked potentially catastrophic command"));

        let input_del = json!({"command": "del /f /q /s c:\\*", "shell": true});
        let err_del = tool.execute(input_del, &ctx()).await.unwrap_err();
        assert!(err_del.to_string().contains("blocked potentially catastrophic command"));
    }

    #[tokio::test]
    async fn allows_relative_cwd_inside_root() {
        let temp = tempfile::tempdir().unwrap();
        let sub = temp.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();

        let tool = CommandTool;
        let root_ctx = ToolContext {
            project_id: "p".into(),
            session_id: None,
            max_permission: PermissionLevel::HighRisk,
            root_path: Some(temp.path().to_path_buf()),
        };

        // Relative cwd "subdir" should resolve inside root and succeed
        let input = json!({"command": "echo inside", "shell": true, "cwd": "subdir"});
        let result = tool.execute(input, &root_ctx).await.unwrap();
        assert_eq!(result.output["success"], json!(true));

        // Relative cwd escaping root with ".." should be rejected
        let bad_input = json!({"command": "echo bad", "shell": true, "cwd": "../escaped"});
        let err = tool.execute(bad_input, &root_ctx).await.unwrap_err();
        assert!(err.to_string().contains("escapes sandbox root"));
    }

    #[test]
    fn tail_does_not_panic_on_multibyte_utf8() {
        // Create a string of Japanese characters (3 bytes each) that exceeds 2000 bytes
        let japanese_str = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをん".repeat(30);
        assert!(japanese_str.len() > OUTPUT_TAIL);
        let trimmed = tail(&japanese_str);
        assert!(!trimmed.is_empty());
    }
}
