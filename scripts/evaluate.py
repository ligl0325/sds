#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
from statistics import mean


def main():
    parser = argparse.ArgumentParser(description="Evaluate SDS retrieval quality")
    parser.add_argument("--queries", required=True)
    parser.add_argument("--k", type=int, default=10)
    parser.add_argument("--bin", default=os.environ.get("SDS_BIN", "sds"))
    args = parser.parse_args()

    cases = json.load(open(args.queries, encoding="utf-8"))
    scores = []
    skipped = 0
    for case in cases:
        expected = set(case.get("relevant_ids", []))
        if not expected:
            skipped += 1
            continue
        output = subprocess.run(
            [args.bin, "search", case["query"], "--top", str(args.k), "--json"],
            check=True,
            capture_output=True,
            text=True,
        )
        results = json.loads(output.stdout)
        ids = [item["id"] for item in results]
        hits = [rank for rank, item_id in enumerate(ids, 1) if item_id in expected]
        scores.append(
            {
                "query": case["query"],
                "recall_at_k": len(set(ids) & expected) / len(expected),
                "mrr": 1.0 / hits[0] if hits else 0.0,
            }
        )

    report = {
        "k": args.k,
        "total_cases": len(cases),
        "labeled_cases": len(scores),
        "skipped_unlabeled": skipped,
        "recall_at_k": mean(item["recall_at_k"] for item in scores) if scores else None,
        "mrr": mean(item["mrr"] for item in scores) if scores else None,
        "cases": scores,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
