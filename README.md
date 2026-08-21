# 闪搜 SDS — Smart Data Search

> 轻量中文记忆 CLI 工具 · Tantivy + jieba 中文分词 · 毫秒级检索

## 核心能力

- **中文全文检索**：Tantivy BM25 + cang-jie 中文分词
- **结构化过滤**：支持 source 与 tags 索引、筛选
- **Agent 友好**：CLI JSON 输出与原生 MCP Server
- **本地优先**：单二进制运行，数据保存在 `~/.sds/`

## 快速开始

```bash
# 存储一条记忆
sds store "Edge AI + Agent AI 交叉领域调研" --source "github/edge-agent" --tags "edge,agent,调研"

# 检索
sds search "Edge AI"

# 按标签过滤
sds search "调研" --tag "github"

# 按来源过滤
sds search "Agent" --source "github"

# 列出最近记忆
sds list --limit 10

# JSON 输出（Agent 友好）
sds search "调研" --json --top 5
```

## 安装

```bash
# 下载二进制到 PATH
cp sds ~/.local/bin/sds
chmod +x ~/.local/bin/sds
```

## 命令参考

### store — 存储记忆

```bash
sds store "内容" --source "来源" --tags "标签1,标签2"
```

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `text` | 记忆文本内容（必填） | — |
| `--source` | 来源标签 | `cli` |
| `--tags` | 逗号分隔标签 | `""` |

### search — 检索记忆

```bash
sds search "关键词" --top 10 --tag "github" --source "cli" --json
```

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `query` | 查询关键词（必填） | — |
| `--top` | 返回结果数 | `10` |
| `--tag` | 按标签过滤（OR 组合） | 不过滤 |
| `--source` | 按来源过滤（全文匹配） | 不过滤 |
| `--json` | JSON 格式输出 | 否 |

### list — 列出记忆

```bash
sds list --limit 10 --offset 0 --source "github" --json
```

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--limit` | 最大条数 | `10` |
| `--offset` | 偏移量 | `0` |
| `--source` | 按来源过滤 | 不过滤 |
| `--json` | JSON 格式输出 | 否 |

### delete — 删除记忆

```bash
sds delete 42
```

### status — 查看状态

```bash
sds status           # 文本格式
sds status --json    # JSON 格式
```

### export — 导出数据

```bash
sds export --format json    # JSON 导出
sds export --format csv     # CSV 导出（含 tags 列）
```

### compact — 分批合并与物理回收

```bash
sds compact
```

将全部可搜索 Segment 以每批 64 段的方式分轮归并，直到只剩 1 段，同时回收旧段文件。命令输出合并前后的 Segment 数、文件数、索引大小、文档数、合并批次和耗时。

SDS自行管理段生命周期：写入提交后若 Segment 数超过 32，会自动执行分批归并，避免CLI逐条写入再次造成长期碎片化。

### migrate — 从 SQLite 导入

```bash
sds migrate /path/to/memory.db
```

兼容包含 `memories(id, text, source, created_at)` 表的 SQLite 数据库。导入过程保留来源字段，并根据来源生成基础标签。

## 并发模型

| 模式 | 句柄 | 命令 | 锁策略 |
|---|---|---|---|
| 只读 | `SdsIndex` | `search` / `list` / `status` / `export` | 不创建 `IndexWriter`，不获取独占锁 |
| 写入 | `SdsWriter` | `store` / `delete` / `compact` / `import` / `migrate` | 持有进程级文件锁和 Tantivy 写锁 |
| MCP | 按请求选择句柄 | `search/status/list_tags` 只读，`store` 写入 | 不在 MCP 进程生命周期内长期占用写锁 |

多个只读进程可以并发查询；同一时间只允许一个写入进程。写入句柄不在 `Drop` 中隐式提交，批量操作必须显式 `commit()`。

## 技术栈

- **引擎**：Tantivy 0.26（Rust 全文检索引擎）
- **分词**：cang-jie（jieba 中文分词）
- **排序**：BM25
- **索引大小**：96.8MB / 9443 条（1个 Segment）
- **实测检索**：约 10ms，RSS 约 6.7MB

## 数据存储

```
~/.sds/
├── counter          # ID 计数器
├── sds.lock         # 文件锁
└── tantivy_index/   # 全文索引
```

## 许可证

MIT