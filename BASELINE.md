# SDS 源码与部署基线

## 当前部署版本

| 项目 | 值 |
|---|---|
| 部署二进制 | `/home/lgl/.local/bin/sds` |
| 源码基线 | `master` 独立根提交 |
| 二进制版本 | `sds 0.3.0` |
| SHA-256 | `5f0fdac618eaa0a84277d1c8704bafb557dd6213bee7085d06b007b7b0eef377` |
| 文件大小 | `6959672` bytes |
| Rust工具链 | `1.96.0` |

## 可复现验证

```bash
cargo build --release
sha256sum target/release/sds /home/lgl/.local/bin/sds
```

项目通过 `rust-toolchain.toml` 固定工具链；Release 构建、Clippy 与测试均在独立 SDS 路径下验证。

## 仓库状态

- 项目目录：`/home/lgl/projects/sds`
- 远端仓库：`github.com/ligl0325/sds`
- 包名、库名、二进制名与代码类型统一使用 `sds` / `Sds*`
- 主分支采用独立根提交，不依赖其他产品身份或迁移历史

## 数据安全

- 独立化前二进制离线备份：`/home/lgl/backups/sds-pre-standalone-binary-e98ef0d.bak`
- 读写拆分前二进制备份：`/home/lgl/backups/sds-pre-readwrite-split-20260821_224243.bak`
- 真合并前二进制备份：`/home/lgl/backups/sds-pre-true-compact-20260822_000437.bak`
- 自动段治理前二进制备份：`/home/lgl/backups/sds-pre-auto-maintenance-20260822_001058.bak`
- IO加固前二进制备份：`/home/lgl/backups/sds-pre-io-hardening-20260822_003000.bak`
- v0.3.0 P0部署前二进制备份：`/home/lgl/backups/sds-pre-v0.3.0-p0-20260822_150856.bak`
- v0.2.0最终部署前二进制备份：`/home/lgl/backups/sds-pre-v0.2.0-final-20260822_081520.bak`
- 真合并前数据备份：`/home/lgl/backups/sds-data-pre-compact-20260821_234611`
- v0.2.0当前数据备份：`/home/lgl/backups/sds-data-v0.2.0-20260822_003500`
- v0.3.0 P0当前数据备份：`/home/lgl/backups/sds-data-v0.3.0-p0-20260822_0815`
- 真实索引已从 9443 个 Segment 合并为 1 个；合并前后 9443 条记录全量导出SHA-256均为 `662e8c6f42d62d267f57f482081bdfb3b2fc2df4a71381c2cc3420b59e5975d9`
