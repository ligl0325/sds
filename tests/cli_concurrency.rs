use std::process::{Command, Stdio};

use tempfile::TempDir;

fn sds_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sds")
}

#[test]
fn test_it_cli_001_four_concurrent_searches_succeed() {
    let home = TempDir::new().expect("应能创建隔离HOME");

    let store = Command::new(sds_bin())
        .args([
            "store",
            "并发检索基线文档",
            "--source",
            "test",
            "--tags",
            "concurrency",
        ])
        .env("HOME", home.path())
        .output()
        .expect("应能执行store");
    assert!(
        store.status.success(),
        "store失败: {}",
        String::from_utf8_lossy(&store.stderr)
    );

    let mut children = Vec::new();
    for _ in 0..4 {
        children.push(
            Command::new(sds_bin())
                .args(["search", "并发检索", "--top", "1", "--json"])
                .env("HOME", home.path())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("应能启动并发search"),
        );
    }

    for child in children {
        let output = child.wait_with_output().expect("应能等待search结束");
        assert!(
            output.status.success(),
            "并发search失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let results: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("search应输出合法JSON");
        assert_eq!(results.as_array().map(Vec::len), Some(1));
    }
}
