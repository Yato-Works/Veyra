use std::path::{Component, Path, PathBuf};
use async_trait::async_trait;
use serde_json::json;

use crate::{
    check_permission, PermissionLevel, RiskLevel, Tool, ToolContext, ToolError, ToolInput,
    ToolManifest, ToolResult,
};

/// Maximum file size allowed to be read into memory (10 MB).
pub const MAX_READ_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum file size allowed to be written in a single call (10 MB).
pub const MAX_WRITE_FILE_SIZE: usize = 10 * 1024 * 1024;

/// Windows reserved DOS device names.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul",
    "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Sensitive operating system and credential paths that must never be accessed.
const SENSITIVE_PATHS: &[&str] = &[
    "c:\\windows",
    "c:\\program files",
    "/etc/passwd",
    "/etc/shadow",
    "/.ssh",
    "\\.ssh",
    "/.aws",
    "\\.aws",
    "/.gnupg",
    "\\.gnupg",
];

/// Returns true if any component of the path matches a Windows reserved device name.
fn is_windows_reserved_device(path: &Path) -> bool {
    for component in path.components() {
        if let Component::Normal(os_str) = component {
            let s = os_str.to_string_lossy().to_lowercase();
            let stem = s.split('.').next().unwrap_or(&s);
            if WINDOWS_RESERVED_NAMES.contains(&stem) {
                return true;
            }
        }
    }
    false
}

/// Normalize path components to eliminate `.` and `..` without requiring the file to exist.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(..) | Component::RootDir => {
                normalized.push(component);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(c) => {
                normalized.push(c);
            }
        }
    }
    normalized
}

/// Resolves and sandboxes a path against the tool context.
///
/// If `ctx.root_path` is specified, the target path MUST resolve inside that root directory.
/// In all cases, known critical operating system directories, credential paths, and Windows
/// reserved device names are blocked.
pub fn resolve_sandboxed_path(path_str: &str, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return Err(ToolError::Failed("fs".into(), "path cannot be empty".into()));
    }

    let input_path = PathBuf::from(trimmed);

    let target_path = if let Some(ref root) = ctx.root_path {
        let abs_root = if root.is_absolute() {
            normalize_path(root)
        } else {
            normalize_path(&std::env::current_dir().unwrap_or_default().join(root))
        };

        let target = if input_path.is_absolute() {
            normalize_path(&input_path)
        } else {
            normalize_path(&abs_root.join(&input_path))
        };

        if !target.starts_with(&abs_root) {
            return Err(ToolError::Failed(
                "fs".into(),
                format!(
                    "access denied: path '{}' escapes sandbox root '{}'",
                    path_str,
                    abs_root.display()
                ),
            ));
        }

        // Validate canonical paths against symlink/junction escapes if target or parent exists on disk
        if let Ok(canonical_root) = std::fs::canonicalize(&abs_root) {
            if target.exists() {
                if let Ok(canonical_target) = std::fs::canonicalize(&target) {
                    if !canonical_target.starts_with(&canonical_root) {
                        return Err(ToolError::Failed(
                            "fs".into(),
                            format!(
                                "access denied: symlink/junction in path '{}' escapes sandbox root '{}'",
                                path_str,
                                abs_root.display()
                            ),
                        ));
                    }
                }
            } else if let Some(parent) = target.parent() {
                if parent.exists() {
                    if let Ok(canonical_parent) = std::fs::canonicalize(parent) {
                        if !canonical_parent.starts_with(&canonical_root) {
                            return Err(ToolError::Failed(
                                "fs".into(),
                                format!(
                                    "access denied: symlink/junction in parent directory of '{}' escapes sandbox root '{}'",
                                    path_str,
                                    abs_root.display()
                                ),
                            ));
                        }
                    }
                }
            }
        }

        target
    } else {
        // Without an explicit root_path, prevent path traversal with relative '..' escaping the working directory
        let base = std::env::current_dir().unwrap_or_default();
        if input_path.is_absolute() {
            normalize_path(&input_path)
        } else {
            let target = normalize_path(&base.join(&input_path));
            if !target.starts_with(&base) {
                return Err(ToolError::Failed(
                    "fs".into(),
                    format!("access denied: relative path '{path_str}' escapes current directory"),
                ));
            }
            target
        }
    };

    // Always block reserved Windows device names to prevent system hangs
    if is_windows_reserved_device(&target_path) {
        return Err(ToolError::Failed(
            "fs".into(),
            format!("access denied: '{path_str}' refers to a reserved Windows device"),
        ));
    }

    // Always block sensitive OS and credential paths (even if inside root_path)
    let target_str = target_path.to_string_lossy().to_lowercase();
    for s in SENSITIVE_PATHS {
        if target_str.contains(s) {
            return Err(ToolError::Failed(
                "fs".into(),
                format!("access denied: access to sensitive system or credential path is restricted: '{path_str}'"),
            ));
        }
    }

    Ok(target_path)
}

/// Tool for reading a UTF-8 text file.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "read_file".into(),
            description: "Reads the content of a text file from the filesystem.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or relative file path to read" }
                },
                "required": ["path"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "size": { "type": "number" }
                }
            }),
            permission: PermissionLevel::Read,
            risk: RiskLevel::None,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Failed("read_file".into(), "missing 'path' argument".into()))?;

        let path = resolve_sandboxed_path(path_str, ctx)?;

        let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
            ToolError::Failed("read_file".into(), format!("failed to read metadata for '{}': {e}", path.display()))
        })?;

        if metadata.len() > MAX_READ_FILE_SIZE {
            return Err(ToolError::Failed(
                "read_file".into(),
                format!(
                    "file '{}' exceeds size limit of 10MB (actual: {} bytes)",
                    path.display(),
                    metadata.len()
                ),
            ));
        }

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            ToolError::Failed("read_file".into(), format!("failed to read '{}': {e}", path.display()))
        })?;

        let size = content.len() as u64;
        Ok(ToolResult {
            output: json!({
                "path": path_str,
                "content": content,
                "size": size
            }),
        })
    }
}

/// Tool for writing or creating a file.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "write_file".into(),
            description: "Writes UTF-8 text content to a file. Overwrites if file exists.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or relative file path to write" },
                    "content": { "type": "string", "description": "Text content to write" }
                },
                "required": ["path", "content"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "bytes_written": { "type": "number" }
                }
            }),
            permission: PermissionLevel::MediumRisk,
            risk: RiskLevel::Medium,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Failed("write_file".into(), "missing 'path' argument".into()))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::Failed("write_file".into(), "missing 'content' argument".into()))?;

        if content.len() > MAX_WRITE_FILE_SIZE {
            return Err(ToolError::Failed(
                "write_file".into(),
                format!(
                    "content exceeds maximum write size of 10MB (actual: {} bytes)",
                    content.len()
                ),
            ));
        }

        let path = resolve_sandboxed_path(path_str, ctx)?;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::Failed(
                        "write_file".into(),
                        format!("failed to create directory '{}': {e}", parent.display()),
                    )
                })?;
            }
        }

        tokio::fs::write(&path, content).await.map_err(|e| {
            ToolError::Failed("write_file".into(), format!("failed to write '{}': {e}", path.display()))
        })?;

        Ok(ToolResult {
            output: json!({
                "path": path_str,
                "bytes_written": content.len()
            }),
        })
    }
}

/// Tool for listing directory entries.
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "list_directory".into(),
            description: "Lists files and subdirectories in a given directory path.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list" }
                },
                "required": ["path"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "entries": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "is_dir": { "type": "boolean" },
                                "size": { "type": "number" }
                            }
                        }
                    }
                }
            }),
            permission: PermissionLevel::Read,
            risk: RiskLevel::None,
        }
    }

    async fn execute(&self, input: ToolInput, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        check_permission(&self.manifest(), ctx)?;

        let path_str = input["path"]
            .as_str()
            .unwrap_or(".");

        let path = resolve_sandboxed_path(path_str, ctx)?;

        let mut read_dir = tokio::fs::read_dir(&path).await.map_err(|e| {
            ToolError::Failed("list_directory".into(), format!("failed to list '{}': {e}", path.display()))
        })?;

        let mut entries = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let metadata = entry.metadata().await.ok();
            let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let name = entry.file_name().to_string_lossy().to_string();

            entries.push(json!({
                "name": name,
                "is_dir": is_dir,
                "size": size
            }));
        }

        // Sort: directories first, then alphabetical
        entries.sort_by(|a, b| {
            let a_dir = a["is_dir"].as_bool().unwrap_or(false);
            let b_dir = b["is_dir"].as_bool().unwrap_or(false);
            match b_dir.cmp(&a_dir) {
                std::cmp::Ordering::Equal => {
                    let an = a["name"].as_str().unwrap_or("");
                    let bn = b["name"].as_str().unwrap_or("");
                    an.cmp(bn)
                }
                other => other,
            }
        });

        Ok(ToolResult {
            output: json!({
                "path": path_str,
                "entries": entries
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fs_tools_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("hello.txt");
        let path_str = file_path.to_str().unwrap().to_string();

        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::MediumRisk,
            root_path: Some(dir.path().to_path_buf()),
        };

        // 1. Write file
        let write_tool = WriteFileTool;
        let write_res = write_tool
            .execute(json!({"path": path_str, "content": "Hello Veyra M6!"}), &ctx)
            .await
            .unwrap();
        assert_eq!(write_res.output["bytes_written"], 15);

        // 2. Read file
        let read_tool = ReadFileTool;
        let read_res = read_tool
            .execute(json!({"path": path_str}), &ctx)
            .await
            .unwrap();
        assert_eq!(read_res.output["content"], "Hello Veyra M6!");

        // 3. List directory
        let list_tool = ListDirTool;
        let list_res = list_tool
            .execute(json!({"path": dir.path().to_str().unwrap()}), &ctx)
            .await
            .unwrap();
        let entries = list_res.output["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["name"], "hello.txt");
        assert_eq!(entries[0]["is_dir"], false);
    }

    #[tokio::test]
    async fn test_sandbox_blocks_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project_root");
        tokio::fs::create_dir(&root).await.unwrap();

        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::MediumRisk,
            root_path: Some(root.clone()),
        };

        let write_tool = WriteFileTool;
        // Attempt to write outside the root using ../
        let result = write_tool
            .execute(
                json!({
                    "path": "../evil.txt",
                    "content": "malicious"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("escapes sandbox root"));
    }

    #[tokio::test]
    async fn test_rejects_windows_reserved_device() {
        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::MediumRisk,
            root_path: None,
        };

        let write_tool = WriteFileTool;
        let result = write_tool
            .execute(
                json!({
                    "path": "CON",
                    "content": "crash"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("reserved Windows device"));
    }

    #[tokio::test]
    async fn test_rejects_sensitive_path_even_with_root() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::MediumRisk,
            root_path: Some(dir.path().to_path_buf()),
        };

        let read_tool = ReadFileTool;
        let result = read_tool
            .execute(
                json!({
                    "path": ".ssh/id_rsa"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("access to sensitive system or credential path is restricted"));
    }

    #[tokio::test]
    async fn test_fs_tools_respect_permission_gate() {
        let ctx = ToolContext {
            project_id: "test".into(),
            session_id: None,
            max_permission: PermissionLevel::Read, // Read only, Write requires MediumRisk
            root_path: None,
        };

        let write_tool = WriteFileTool;
        let result = write_tool
            .execute(
                json!({
                    "path": "test.txt",
                    "content": "write"
                }),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::PermissionDenied { .. })));
    }
}
