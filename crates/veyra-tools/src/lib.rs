//! Veyra Tool Runtime: capabilities exposed to agents
//! (Plan.txt sections 24-26 and 45-46).
//!
//! Every Tool provides a manifest (name, description, schemas, permission
//! and risk level) and is gated by a permission check before execution.
//! An Agent should never gain unrestricted access merely because a tool
//! exists.

mod fs;
mod process;
mod registry;
mod windows;

pub use fs::{normalize_path, resolve_sandboxed_path, ListDirTool, ReadFileTool, WriteFileTool};
pub use process::CommandTool;
pub use registry::ToolRegistry;
pub use windows::{ActiveWindowTool, LaunchAppTool, OpenBrowserTool};

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Permission levels (Plan.txt section 45), ordered from least to most
/// privileged. Examples:
///
/// - Read file → `Read`
/// - Create file / run build → `LowRisk`
/// - Modify source → `MediumRisk`
/// - Delete files / send email → `HighRisk`
/// - Purchase → `Critical`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    Read,
    LowRisk,
    MediumRisk,
    HighRisk,
    Critical,
}

/// Coarse risk classification shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    None,
    Low,
    Medium,
    High,
}

/// Self-describing metadata every tool must provide (Plan.txt section 24).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub permission: PermissionLevel,
    pub risk: RiskLevel,
}

/// Input to a tool call, validated against the manifest's input schema.
pub type ToolInput = Value;

/// Execution context handed to every tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub project_id: String,
    pub session_id: Option<String>,
    /// Maximum permission level the current approval allows.
    pub max_permission: PermissionLevel,
    /// Root path allowed for file operations (optional sandbox root).
    #[serde(default)]
    pub root_path: Option<PathBuf>,
}

impl ToolContext {
    pub fn new(project_id: impl Into<String>, max_permission: PermissionLevel) -> Self {
        Self {
            project_id: project_id.into(),
            session_id: None,
            max_permission,
            root_path: None,
        }
    }

    pub fn with_root_path(mut self, root_path: impl Into<PathBuf>) -> Self {
        self.root_path = Some(root_path.into());
        self
    }
}

/// Successful tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: Value,
}

/// Tool errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("permission denied: requires {required:?}, allowed {allowed:?}")]
    PermissionDenied {
        required: PermissionLevel,
        allowed: PermissionLevel,
    },
    #[error("tool '{0}' failed: {1}")]
    Failed(String, String),
}

/// The Tool interface (Plan.txt section 24).
#[async_trait]
pub trait Tool: Send + Sync {
    fn manifest(&self) -> ToolManifest;

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError>;
}

/// Permission gate: refuse any tool call that exceeds the context's allowed
/// permission level (Plan.txt section 46, Execution Policy).
pub fn check_permission(
    manifest: &ToolManifest,
    ctx: &ToolContext,
) -> Result<(), ToolError> {
    if manifest.permission > ctx.max_permission {
        Err(ToolError::PermissionDenied {
            required: manifest.permission,
            allowed: ctx.max_permission,
        })
    } else {
        Ok(())
    }
}

/// A minimal reference tool used in tests: echoes its input back.
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "echo".into(),
            description: "Echoes the provided input.".into(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: serde_json::json!({"type": "object"}),
            permission: PermissionLevel::Read,
            risk: RiskLevel::None,
        }
    }

    async fn execute(&self, input: ToolInput, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult { output: input })
    }
}

/// Creates a default ToolRegistry populated with core tools (fs, command, windows, echo).
pub fn default_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    registry.register(Arc::new(CommandTool));
    registry.register(Arc::new(ReadFileTool));
    registry.register(Arc::new(WriteFileTool));
    registry.register(Arc::new(ListDirTool));
    registry.register(Arc::new(LaunchAppTool));
    registry.register_alias("launch_app", Arc::new(LaunchAppTool));
    registry.register(Arc::new(OpenBrowserTool));
    registry.register_alias("open_url", Arc::new(OpenBrowserTool));
    registry.register_alias("browser", Arc::new(OpenBrowserTool));
    registry.register(Arc::new(ActiveWindowTool));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_levels_are_ordered() {
        assert!(PermissionLevel::Read < PermissionLevel::LowRisk);
        assert!(PermissionLevel::LowRisk < PermissionLevel::MediumRisk);
        assert!(PermissionLevel::MediumRisk < PermissionLevel::HighRisk);
        assert!(PermissionLevel::HighRisk < PermissionLevel::Critical);
    }

    #[test]
    fn permission_gate_blocks_overprivileged_calls() {
        let manifest = ToolManifest {
            name: "delete_file".into(),
            description: "Deletes a file.".into(),
            input_schema: json!({}),
            output_schema: json!({}),
            permission: PermissionLevel::HighRisk,
            risk: RiskLevel::High,
        };

        let permissive = ToolContext {
            project_id: "p".into(),
            session_id: None,
            max_permission: PermissionLevel::Critical,
            root_path: None,
        };
        let restrictive = ToolContext {
            project_id: "p".into(),
            session_id: None,
            max_permission: PermissionLevel::LowRisk,
            root_path: None,
        };

        assert!(check_permission(&manifest, &permissive).is_ok());
        assert_eq!(
            check_permission(&manifest, &restrictive).unwrap_err(),
            ToolError::PermissionDenied {
                required: PermissionLevel::HighRisk,
                allowed: PermissionLevel::LowRisk,
            }
        );
    }

    #[tokio::test]
    async fn echo_tool_executes() {
        let ctx = ToolContext {
            project_id: "p".into(),
            session_id: None,
            max_permission: PermissionLevel::Read,
            root_path: None,
        };
        let input = json!({"message": "こんにちは"});
        let result = EchoTool.execute(input.clone(), &ctx).await.unwrap();
        assert_eq!(result.output, input);
    }

    #[test]
    fn default_registry_contains_initial_tools() {
        let reg = default_tool_registry();
        let manifests = reg.list_manifests();
        let names: Vec<_> = manifests.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"execute_command"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"list_directory"));
        assert!(names.contains(&"launch_application"));
        assert!(names.contains(&"active_window"));
    }
}
