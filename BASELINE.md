# SDS 源码与部署基线

## 当前部署版本

| 项目 | 值 |
|---|---|
| 部署二进制 | `/home/lgl/.local/bin/sds` |
| 源码基线 | `master` 独立根提交 |
| 二进制版本 | `sds 0.1.0` |
| SHA-256 | `887b26b31976356334c50e3268c32c3a1664259ddfcff37f918156995f565fb8` |
| 文件大小 | `6788248` bytes |
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

- 原部署二进制离线备份：`/home/lgl/backups/sds-pre-standalone-binary-e98ef0d.bak`
- 本次未修改 `/home/lgl/.sds/` 中的真实索引和记忆数据
