# SDS检索评测集

`queries.json` 使用如下格式：

```json
[
  {
    "query": "输入法",
    "relevant_ids": [123, 456],
    "note": "人工确认的相关记忆ID"
  }
]
```

运行评测：

```bash
python3 scripts/evaluate.py --queries eval/queries.json --k 10
```

输出指标：

- `recall_at_k`：前K条是否覆盖人工标注的相关记忆
- `mrr`：第一条相关结果的倒数排名
- `labeled_cases`：已经完成标注的查询数

禁止直接把SDS当前Top结果复制为`relevant_ids`，那会把检索结果当成标准答案。先人工标注，再比较BM25、重要性和时间衰减策略。
