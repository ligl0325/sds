# SDS真实检索标注队列

本目录中的 `queries.json` 是本机生成的候选队列，默认被 `.gitignore` 排除，不上传GitHub。

## 标注方式

打开 `eval/queries.json`，为每条查询填写：

```json
"relevant_ids": [123, 456]
```

规则：

- 相关：能直接回答查询、提供事实依据或对当前任务有明确帮助
- 不相关：只是共享关键词，但不能回答查询
- 多条相关结果全部填写
- 不确定时不要猜，留空并在`note`中说明
- 不要把当前Top结果默认当成标准答案

## 重新生成候选

```bash
python3 scripts/prepare_eval.py --top 10
```

## 计算指标

```bash
python3 scripts/evaluate.py --queries eval/queries.json --k 10
```
