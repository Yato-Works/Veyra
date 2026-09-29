//! Central registry for tools available to Veyra agents and assistants
//! (Plan.txt sections 24-25).

use std::collections::HashMap;
use std::sync::Arc;

use crate::{check_permission, Tool, ToolContext, ToolError, ToolInput, ToolManifest, ToolResult};

/// Stores and coordinates tools exposed to runtime agents.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Replaces any existing tool registered under the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.manifest().name;
        self.tools.insert(name, tool);
    }

    /// Register an alias for a tool under an alternative name.
    pub fn register_alias(&mut self, alias: &str, tool: Arc<dyn Tool>) {
        self.tools.insert(alias.to_string(), tool);
    }

    /// Retrieve a registered tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Return manifests for all registered tools (useful for LLM tool selection prompts).
    pub fn list_manifests(&self) -> Vec<ToolManifest> {
        let mut list: Vec<_> = self.tools.values().map(|t| t.manifest()).collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list.dedup_by(|a, b| a.name == b.name);
        list
    }

    /// Execute a tool with permission checking.
    pub async fn execute(
        &self,
        name: &str,
        input: ToolInput,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tool = self.tools.get(name).ok_or_else(|| {
            ToolError::Failed(name.to_string(), format!("tool '{name}' not found in registry"))
        })?;

        let manifest = tool.manifest();
        check_permission(&manifest, ctx)?;

        tool.execute(input, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EchoTool, PermissionLevel, RiskLevel};
    use serde_json::json;

    #[tokio::test]
    async fn registry_registers_and_executes_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool));

        let manifests = registry.list_manifests();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "echo");

        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::Read,
            root_path: None,
        };

        let res = registry
            .execute("echo", json!({"hello": "world"}), &ctx)
            .await
            .unwrap();
        assert_eq!(res.output, json!({"hello": "world"}));
    }

    #[tokio::test]
    async fn registry_respects_permissions() {
        struct RiskyTool;
        #[async_trait::async_trait]
        impl Tool for RiskyTool {
            fn manifest(&self) -> ToolManifest {
                ToolManifest {
                    name: "danger".into(),
                    description: "High risk operation".into(),
                    input_schema: json!({}),
                    output_schema: json!({}),
                    permission: PermissionLevel::HighRisk,
                    risk: RiskLevel::High,
                }
            }
            async fn execute(
                &self,
                _input: ToolInput,
                _ctx: &ToolContext,
            ) -> Result<ToolResult, ToolError> {
                Ok(ToolResult { output: json!("done") })
            }
        }

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(RiskyTool));

        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::LowRisk,
            root_path: None,
        };

        let err = registry.execute("danger", json!({}), &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied { .. }));
    }
}
