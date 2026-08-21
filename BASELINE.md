# SDS 源码与部署基线

## 当前部署版本

| 项目 | 值 |
|---|---|
| 部署二进制 | `/home/lgl/.local/bin/sds` |
| 源码基线 | `master` 独立根提交 |
| 二进制版本 | `sds 0.1.0` |
| SHA-256 | `b25c4534610f167da62b10614e217a2e50cfc4598c2b43d346dcf832828e46e8` |
| 文件大小 | `6790008` bytes |
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
- 本次未修改 `/home/lgl/.sds/` 中的真实索引和记忆数据
