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
# 从GitHub最新Release安装Linux x86_64版本
curl --fail --location https://raw.githubusercontent.com/ligl0325/sds/master/scripts/install.sh | bash

# 安装指定版本
SDS_VERSION=v0.2.0 curl --fail --location https://raw.githubusercontent.com/ligl0325/sds/master/scripts/install.sh | bash
```

安装脚本会校验Release压缩包SHA-256，并同时安装 `sds` 与 `sds-mcp` 到 `~/.local/bin/`。手动安装仍可直接复制二进制：

```bash
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

正常的 `AND` / `OR` / 短语查询语法保持可用；若用户输入包含未闭合括号、引号、路径等非法查询语法，SDS会自动退化为安全字面量查询。纯标点查询返回空结果，不会误召回全库。

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

JSON状态包含 `memories`、`segments`、`files`、`fragmentation_rate` 和 `schema_version`。碎片率定义为 `(segments - 1) / memories × 100%`，越接近0越健康。

### backup / restore — 备份与恢复

```bash
sds backup /path/to/sds-backup
sds restore /path/to/sds-backup --verify
```

`backup`要求目标目录不存在，复制前会先提交待写入内容并校验备份索引。`restore --verify`会先校验备份，再用暂存目录替换当前数据；原数据会保留为 `.sds.pre-restore-*`，恢复后再次校验。

### benchmark — 标准检索基准

```bash
sds benchmark --query "Hermes" --repeat 20 --top 10 --json
```

输出 `min/p50/p95/max/avg` 延迟、结果数和当前进程RSS，默认先做一次预热，不包含CLI进程启动时间。

### export — 导出数据

```bash
sds export --format json    # JSON 导出
sds export --format csv     # 标准 RFC 4180 CSV（含 tags 列）
```

CSV由专用序列化器生成，正确处理逗号、双引号和字段内换行。

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

## Schema版本

当前Schema为v1，版本记录在 `~/.sds/schema_version`。没有版本文件的早期索引按兼容v1读取；未来遇到高于当前程序的版本会拒绝写入并提示升级，避免静默破坏数据。

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