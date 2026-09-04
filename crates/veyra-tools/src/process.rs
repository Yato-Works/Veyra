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
const OUTPUT_TAIL: usize = 2000;

/// Run a shell command and capture its exit status and output.
pub struct CommandTool;

fn tail(s: &str) -> String {
    let s = s.trim_end();
    if s.len() <= OUTPUT_TAIL {
        s.to_string()
    } else {
        let cut = s.len() - OUTPUT_TAIL;
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
            // Plan.txt section 45: "Run build → LOW_RISK".
            permission: PermissionLevel::LowRisk,
            risk: RiskLevel::Medium,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let command = input["command"].as_str().ok_or_else(|| {
            ToolError::Failed("execute_command".into(), "missing 'command'".into())
        })?;
        let shell = input["shell"].as_bool().unwrap_or(false);
        let timeout_secs = input["timeout_secs"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

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

        if let Some(cwd) = input["cwd"].as_str() {
            cmd.current_dir(cwd);
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
            max_permission: PermissionLevel::MediumRisk,
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
            max_permission: PermissionLevel::Read,
        };
        let input = json!({"command": "exit 0", "shell": true});
        let err = tool.execute(input, &restrictive).await.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied { .. }));
    }
}
