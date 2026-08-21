//! SDS MCP Server — 让任何 AI 助手都能搜索你的本地中文知识库
//!
//! 协议：MCP (Model Context Protocol) over stdio
//! 格式：JSON-RPC 2.0
//!
//! 使用方式：
//!   1. 编译：cargo build --release --bin sds-mcp
//!   2. 配置到 Claude Desktop：
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "sds": {
//!       "command": "/path/to/sds-mcp"
//!     }
//!   }
//! }
//! ```

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// 复用 SDS 的 index 模块
use sds::index::SdsIndex;

// ── MCP 协议常量 ──

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "sds-mcp";
const SERVER_VERSION: &str = "0.1.0";

// ── JSON-RPC 2.0 消息结构 ──

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

// ── MCP 工具定义 ──

#[derive(Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    #[serde(rename = "inputSchema", skip_serializing_if = "Option::is_none")]
    input_schema: Option<Value>,
}

fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search".into(),
            description: "搜索本地知识库，支持中文分词和标签/来源过滤".into(),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词"
                    },
                    "top": {
                        "type": "number",
                        "description": "返回条数，默认 10",
                        "default": 10
                    },
                    "tag": {
                        "type": "string",
                        "description": "按标签过滤（可选）"
                    },
                    "source": {
                        "type": "string",
                        "description": "按来源过滤（可选）"
                    }
                },
                "required": ["query"]
            })),
        },
        ToolDefinition {
            name: "store".into(),
            description: "存入一条文档到知识库，自动建索引".into(),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "文档内容"
                    },
                    "source": {
                        "type": "string",
                        "description": "来源，默认 mcp"
                    },
                    "tags": {
                        "type": "string",
                        "description": "逗号分隔的标签"
                    }
                },
                "required": ["text"]
            })),
        },
        ToolDefinition {
            name: "status".into(),
            description: "查询索引状态：文档总数、索引大小、路径".into(),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {}
            })),
        },
        ToolDefinition {
            name: "list_tags".into(),
            description: "列出所有标签".into(),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {}
            })),
        },
    ]
}

// ── 工具执行 ──

fn execute_tool(index: &mut SdsIndex, name: &str, args: &Value) -> Result<Value> {
    match name {
        "search" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("search: 缺少 query 参数"))?;
            let top = args["top"].as_u64().unwrap_or(10) as usize;
            let tag = args["tag"].as_str();
            let source = args["source"].as_str();

            let results = index.search(query, top, tag, source)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string_pretty(&results)?
                    }
                ]
            }))
        }
        "store" => {
            let text = args["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("store: 缺少 text 参数"))?;
            let source = args["source"].as_str().unwrap_or("mcp");
            let tags = args["tags"].as_str().unwrap_or("");

            let mem = index.store(text, source, tags)?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": format!("已存入文档 #{}\n来源: {}\n标签: {}", mem.id, mem.source, mem.tags)
                    }
                ]
            }))
        }
        "status" => {
            let status = index.status()?;
            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": format!(
                            "文档总数: {}\n索引大小: {}\n索引路径: {}",
                            status.memories, status.index_size, status.index_path
                        )
                    }
                ]
            }))
        }
        "list_tags" => {
            // 从所有文档中收集唯一标签
            let all = index.all()?;
            let mut tags_set: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for mem in &all {
                for tag in mem.tags.split(',') {
                    let t = tag.trim();
                    if !t.is_empty() {
                        tags_set.insert(t.to_string());
                    }
                }
            }
            let mut tags: Vec<String> = tags_set.into_iter().collect();
            tags.sort();

            let mut result = String::new();
            for (i, tag) in tags.iter().enumerate() {
                result.push_str(&format!("{}. {}\n", i + 1, tag));
            }
            if result.is_empty() {
                result = "暂无标签".to_string();
            }

            Ok(serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": result
                    }
                ]
            }))
        }
        _ => Err(anyhow::anyhow!("未知工具: {}", name)),
    }
}

// ── 数据目录 ──

fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".sds")
}

// ── 主循环 ──

fn main() -> Result<()> {
    // 打开索引
    let sds_dir = data_dir();
    std::fs::create_dir_all(&sds_dir)?;

    // 设置权限锁（仅当前用户可读）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&sds_dir) {
            let mut perms = meta.permissions();
            perms.set_mode(0o700);
            let _ = std::fs::set_permissions(&sds_dir, perms);
        }
    }

    let mut index = SdsIndex::open(&sds_dir)?;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    // 主循环：逐行读取 JSON-RPC 请求
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp)?);
                let _ = stdout.flush();
                continue;
            }
        };

        let response = handle_request(&mut index, &request);
        let resp_str = serde_json::to_string(&response)?;
        let _ = writeln!(stdout, "{}", resp_str);
        let _ = stdout.flush();
    }

    Ok(())
}

fn handle_request(index: &mut SdsIndex, request: &JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone();

    match request.method.as_str() {
        // ── MCP 初始化 ──
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            })),
            error: None,
        },

        // ── 初始化通知（无需响应） ──
        "notifications/initialized" => {
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: None, // 通知无 id
                result: None,
                error: None,
            }
        }

        // ── 列出工具 ──
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(serde_json::json!({
                "tools": tool_definitions()
            })),
            error: None,
        },

        // ── 调用工具 ──
        "tools/call" => {
            let empty_obj = serde_json::json!({});
            let params = request.params.as_ref().unwrap_or(&empty_obj);
            let tool_name = params["name"].as_str().unwrap_or("");
            let arguments = params.get("arguments").unwrap_or(&empty_obj);

            match execute_tool(index, tool_name, arguments) {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(result),
                    error: None,
                },
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32603,
                        message: format!("Tool execution error: {}", e),
                        data: None,
                    }),
                },
            }
        }

        // ── 未知方法 ──
        _ => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        },
    }
}
