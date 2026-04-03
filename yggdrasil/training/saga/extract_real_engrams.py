#!/usr/bin/env python3
"""Extract real engrams from Mimir and convert to saga training data.

Semi-synthetic approach: real cause/effect pairs from production memory,
categorized and formatted to match saga's exact schema.
"""
import json
import requests
from collections import Counter

MIMIR_URL = "http://10.0.65.9:9090"
SYSTEM = "You are Saga, Yggdrasil's memory engine. Respond ONLY in valid JSON."

QUERIES = [
    "bug fix resolved patched corrected error",
    "architecture decision design refactor split",
    "sprint started completed planning review",
    "deployment change deployed config updated service",
    "gotcha non-obvious workaround discovered issue",
    "user feedback preference correction request",
    "infrastructure network hardware GPU memory",
    "model training fine-tuning benchmark evaluation",
    "memory engram store query recall pipeline",
    "agent tool loop pipeline flow execution",
    "voice audio STT TTS speech recognition",
    "session config routing backend proxy",
    "test harness integration validation",
    "security sanitize token auth credential",
]


def pull_engrams():
    all_engrams = []
    seen = set()

    for q in QUERIES:
        try:
            resp = requests.post(
                f"{MIMIR_URL}/api/v1/recall",
                json={"text": q, "limit": 20, "include_text": True},
                timeout=5,
            )
            if resp.ok:
                for e in resp.json().get("events", []):
                    cause = e.get("cause", "")
                    effect = e.get("effect", "")
                    if cause and effect and len(cause) > 20 and len(effect) > 20:
                        key = cause[:80]
                        if key not in seen:
                            seen.add(key)
                            all_engrams.append({
                                "cause": cause,
                                "effect": effect,
                                "tags": e.get("tags", []),
                            })
        except Exception as ex:
            print(f"  Query '{q[:30]}...' failed: {ex}")

    return all_engrams


def categorize(engram):
    text = (engram["cause"] + " " + engram["effect"]).lower()
    tags = [t.lower() for t in engram.get("tags", [])]

    if any(t in tags for t in ["bug_fix"]):
        return "bug_fix"
    if any(t in tags for t in ["architecture_decision"]):
        return "architecture_decision"
    if any(t in tags for t in ["sprint_lifecycle", "sprint"]):
        return "sprint_lifecycle"
    if any(t in tags for t in ["deployment_change"]):
        return "deployment_change"
    if any(t in tags for t in ["gotcha"]):
        return "gotcha"
    if any(t in tags for t in ["user_feedback"]):
        return "user_feedback"

    # Heuristic fallback
    first50 = text[:50]
    if "fixed" in first50 or "bug" in first50 or "panic" in first50 or "crash" in first50:
        return "bug_fix"
    if "sprint" in first50:
        return "sprint_lifecycle"
    if "deploy" in first50 or "service" in first50 or "systemd" in first50:
        return "deployment_change"
    if "gotcha" in text or "non-obvious" in text or "workaround" in text:
        return "gotcha"
    if "user:" in first50 or "feedback" in first50:
        return "user_feedback"
    if "refactor" in first50 or "added" in first50 or "created" in first50 or "architecture" in first50:
        return "architecture_decision"

    return "architecture_decision"  # default for complex technical content


def to_training_examples(engrams):
    examples = []
    for e in engrams:
        cat = categorize(e)
        cause = e["cause"][:300].strip()
        effect = e["effect"][:500].strip()
        tags = e.get("tags", [cat])[:5]

        # CLASSIFY
        user_classify = f"CLASSIFY\ntool: Edit\nfile: \ncontent: {cause}"
        examples.append({"messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user_classify},
            {"role": "assistant", "content": json.dumps({
                "category": cat,
                "should_store": True,
                "confidence": 0.92,
            })},
        ]})

        # DISTILL with REAL cause/effect
        user_distill = f"DISTILL\ntool: Edit\nfile: \ncontent: {cause}"
        examples.append({"messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user_distill},
            {"role": "assistant", "content": json.dumps({
                "cause": cause[:200],
                "effect": effect[:300],
                "tags": tags,
            })},
        ]})

    return examples


def main():
    print("Pulling engrams from Mimir...")
    engrams = pull_engrams()
    print(f"  Got {len(engrams)} unique engrams")

    examples = to_training_examples(engrams)

    # Count categories
    cats = Counter()
    for e in examples:
        if "CLASSIFY" in e["messages"][1]["content"]:
            resp = json.loads(e["messages"][2]["content"])
            cats[resp.get("category", "unknown")] += 1

    outpath = "data/saga_real_engrams.jsonl"
    with open(outpath, "w") as f:
        for ex in examples:
            f.write(json.dumps(ex) + "\n")

    print(f"  Generated {len(examples)} training examples")
    print(f"  Categories: {dict(sorted(cats.items()))}")
    print(f"  Output: {outpath}")


if __name__ == "__main__":
    main()
