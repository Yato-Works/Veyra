//! Context Package (Plan.txt §18).
//!
//! The Context Engine determines what an AI should see.
//! Never send the entire project history blindly; provide a structured package
//! of authoritative facts.

use serde::{Deserialize, Serialize};
use crate::{Goal, ProjectState};

/// Structured context package handed to AI models (Plan.txt §18).
/// Replaces raw conversation history with relevant, prioritized runtime facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackage {
    pub project_id: String,
    pub user_instruction: String,
    pub active_goal: Option<Goal>,
    pub project_state: Option<ProjectState>,
    pub recent_actions: Vec<String>,
    pub active_application: Option<String>,
}

impl ContextPackage {
    pub fn new(project_id: impl Into<String>, instruction: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            user_instruction: instruction.into(),
            active_goal: None,
            project_state: None,
            recent_actions: Vec::new(),
            active_application: None,
        }
    }

    /// Formats the context package into a compact, human-readable section for LLM prompts.
    pub fn format_prompt_section(&self) -> String {
        let mut out = String::new();
        out.push_str("【コンテキスト情報 (Plan §18)】\n");

        if let Some(ref goal) = self.active_goal {
            out.push_str(&format!("- 現在の目標: 「{}」 (状態: {:?})\n", goal.objective, goal.status));
        } else {
            out.push_str("- 現在進行中の目標: なし\n");
        }

        if let Some(ref app) = self.active_application {
            out.push_str(&format!("- 最前面のアクティブウィンドウ: 「{}」\n", app));
        }

        if !self.recent_actions.is_empty() {
            out.push_str("- 直近の動作履歴:\n");
            for act in &self.recent_actions {
                out.push_str(&format!("  * {}\n", act));
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_package_serializes_and_formats() {
        let mut ctx = ContextPackage::new("p1", "test instruction");
        ctx.active_application = Some("Chrome - YouTube".into());
        ctx.recent_actions.push("ツール実行完了: open_browser".into());

        let prompt = ctx.format_prompt_section();
        assert!(prompt.contains("Chrome - YouTube"));
        assert!(prompt.contains("open_browser"));

        let json = serde_json::to_value(&ctx).unwrap();
        assert_eq!(json["project_id"], "p1");
    }
}
