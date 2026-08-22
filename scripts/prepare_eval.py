#!/usr/bin/env python3
import argparse
import json
import os
import subprocess


def main():
    parser = argparse.ArgumentParser(description="Prepare local SDS labeling queue")
    parser.add_argument("--seeds", default="eval/query_seeds.json")
    parser.add_argument("--output", default="eval/queries.json")
    parser.add_argument("--top", type=int, default=10)
    parser.add_argument("--bin", default=os.environ.get("SDS_BIN", "sds"))
    args = parser.parse_args()

    seeds = json.load(open(args.seeds, encoding="utf-8"))
    cases = []
    for query in seeds:
        process = subprocess.run(
            [args.bin, "search", query, "--top", str(args.top), "--json"],
            check=True,
            capture_output=True,
            text=True,
        )
        results = json.loads(process.stdout)
        candidates = [
            {
                "id": item["id"],
                "source": item.get("source", ""),
                "tags": item.get("tags", ""),
                "memory_type": item.get("memory_type", "legacy"),
                "importance": item.get("importance", 50),
                "preview": item.get("text", "")[:160],
            }
            for item in results
        ]
        cases.append({"query": query, "relevant_ids": [], "candidates": candidates})

    output = args.output
    os.makedirs(os.path.dirname(output) or ".", exist_ok=True)
    with open(output, "w", encoding="utf-8") as handle:
        json.dump(cases, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    print(json.dumps({"queries": len(cases), "output": output, "top": args.top}, ensure_ascii=False))


if __name__ == "__main__":
    main()
