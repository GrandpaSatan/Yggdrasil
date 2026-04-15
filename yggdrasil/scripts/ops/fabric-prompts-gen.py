#!/usr/bin/env python3
"""
Yggdrasil Shared Memory Fabric — Prompt corpus generator.
Sprint 069 Phase G.5 support.

Generates a seeded, reproducible JSONL corpus of ~3,000 prompts
covering the distribution the fabric's L2 projections need to learn:
code-swarm, memory-consolidation, and general-reasoning domains.

Runs offline. No external dataset downloads. Seeded with Python's
random.Random(42) so the same corpus is produced on any workstation.

Output: JSONL, one record per line:
    {"prompt": "...", "source": "code-swarm|mem|general", "tokens_est": int}
"""

import argparse
import hashlib
import json
import random
from pathlib import Path

SEED = 42

# ───────────────────────── Code-swarm prompts ─────────────────────────
CODE_VERBS = ["implement", "write", "refactor", "optimize", "debug", "review",
              "explain", "document", "test"]
CODE_TARGETS = [
    "a Rust axum handler for",
    "a Python async function that",
    "a Go HTTP server with",
    "a React component for",
    "a SQL migration adding",
    "a systemd unit that starts",
    "a pytest fixture for",
    "a vitest suite covering",
    "a bash script to",
    "a JSON schema describing",
]
CODE_SUBJECTS = [
    "user authentication with JWT rotation",
    "paginated list retrieval with cursor encoding",
    "a vector embedding cache with TTL eviction",
    "webhook signature verification via HMAC-SHA256",
    "a rate limiter with sliding-window counters",
    "a worker pool consuming from a Redis stream",
    "graceful shutdown handling SIGTERM and SIGINT",
    "model checkpoint restoration with gradient accumulation",
    "TLS certificate pinning for a Proxmox client",
    "a PostgreSQL connection pool with health checks",
    "a binary search tree with in-order traversal",
    "a DNS resolver honouring /etc/hosts first",
    "a Kafka consumer with at-least-once semantics",
    "ONNX inference wrapped in a thin HTTP service",
    "a Prometheus histogram with custom buckets",
    "a Merkle-tree builder with proof generation",
    "a sliding-window TTFT benchmark harness",
    "an LRU cache with invalidation by tag",
    "a protocol buffers schema for a heartbeat service",
    "a CORS-compliant middleware layer",
]
CODE_CONTEXTS = [
    "Use only the standard library where possible.",
    "The handler must return 400 on validation errors.",
    "Assume the database schema is fixed — do not alter it.",
    "Follow the existing repo conventions for error handling.",
    "The function runs in a hot path; prefer zero-allocation paths.",
    "Include a minimal example of how to call it.",
    "The service must gracefully degrade if the backend is unavailable.",
    "Preserve backwards compatibility with clients on v1.",
    "Log at DEBUG on success, INFO on first failure, ERROR on retry exhaustion.",
    "Emit Prometheus metrics for request count and latency.",
]

# ───────────────────────── Memory-consolidation prompts ─────────────────────────
MEM_TEMPLATES = [
    "Summarize the following notes about {topic}, preserving the causal chain: {facts}",
    "Extract the key decisions made regarding {topic}: {facts}",
    "Merge these observations about {topic} into a single coherent paragraph: {facts}",
    "Given these prior beliefs about {topic}: {facts}\nDetermine whether the new observation '{fact}' is NEW, UPDATE, or OLD.",
    "What pattern emerges from the following interactions about {topic}?\n{facts}",
    "Reconcile the following possibly conflicting statements about {topic}: {facts}",
]
MEM_TOPICS = [
    "the inference fleet latency profile",
    "the swarm's dream-cycle outcomes over the last 24 hours",
    "recent security audit findings",
    "cross-model KV reuse hit rates per pair",
    "the vault's secret-rotation history",
    "this sprint's open carry-overs",
    "the memory subsystem's precision/recall trend",
    "user-initiated corrections in the last week",
    "GPU thermal events on Hugin",
    "flow-step error modes observed in production",
]
MEM_FACTS_POOL = [
    "Saga-350m's TTFT dropped 15% after the KV cache warm-up hook landed.",
    "Review-1.2b is now the default reviewer in every swarm pair.",
    "lfm25-tools fails to parse nested tool-calls deeper than 3.",
    "Qwen2.5-0.5B-Instruct is used as the baseline-generic backbone.",
    "The dreamer consolidated 42 engrams into 7 summaries overnight.",
    "VULN-006 mesh handshake is now PSK-gated via HMAC-SHA256.",
    "The fabric's L3 tier hits 67% on same-flow queries and 12% cross-flow.",
    "Morrigan trains projections in ~4 hours with both RTX 3060s active.",
    "Hugin's llama-swap serves 4 models and auto-evicts after 300 s idle.",
    "TEI on Munin 780M returns 384-dim all-MiniLM embeddings at ~110 doc/s.",
]

# ───────────────────────── General-reasoning prompts ─────────────────────────
GEN_TEMPLATES = [
    "Explain why {phenomenon} happens, in terms a non-specialist can follow.",
    "Compare and contrast {a} and {b}, naming three concrete differences.",
    "Given these constraints: {constraints}\nWhat is the best approach to {goal}?",
    "Is it true that {claim}? Justify your answer with evidence or a counterexample.",
    "If {premise}, what follows about {question}?",
    "Describe the steps you would take to {goal}, assuming {context}.",
    "What are the most common failure modes of {system}, and how do you detect each?",
    "Why might a practitioner prefer {option_a} over {option_b} when {condition}?",
]
GEN_PHENOMENA = [
    "floating-point accumulation error grows with batch size",
    "HTTP/2 hops of less than a millisecond can dominate total request latency",
    "LoRA adapters can preserve base-model behavior while specializing",
    "a systemd timer drifts even when clock sync is enabled",
    "tokenizer choice changes downstream model throughput",
    "AMD iGPU memory is partitioned from system RAM on boot",
    "attention Q·K matmul dominates prefill time",
    "rotary position embeddings generalize length better than absolute positions",
    "prefix caching helps multi-turn chat but hurts throughput under high load",
    "knowledge distillation sometimes improves calibration over the teacher",
]
GEN_COMPARE_PAIRS = [
    ("HNSW", "IVFFlat"),
    ("bearer tokens", "mTLS"),
    ("Qdrant", "pgvector"),
    ("axum", "actix-web"),
    ("GGUF", "safetensors"),
    ("vLLM prefix caching", "LMCache tiered offload"),
    ("CPU-float baselines", "GPU-mixed-precision runs"),
    ("systemd .path units", "filesystem inotify watchers"),
    ("bearer-token middleware", "signed-request middleware"),
    ("mDNS service discovery", "static IP configuration"),
]
GEN_GOALS = [
    "roll out a schema migration with zero downtime",
    "detect a regression in reranker nDCG@10",
    "harden a webhook endpoint against replay attacks",
    "profile a hot path without adding production latency",
    "select an HNSW M value for a 1M-vector collection",
    "decide whether to fork an upstream plugin or wait for a release",
    "tune gpu_memory_utilization on a constrained iGPU",
    "choose between an LRU and a semantic-similarity eviction policy",
]


def gen_code_swarm(rng: random.Random) -> str:
    verb = rng.choice(CODE_VERBS)
    target = rng.choice(CODE_TARGETS)
    subject = rng.choice(CODE_SUBJECTS)
    context = rng.choice(CODE_CONTEXTS)
    return f"{verb.capitalize()} {target} {subject}. {context}"


def gen_memory(rng: random.Random) -> str:
    template = rng.choice(MEM_TEMPLATES)
    topic = rng.choice(MEM_TOPICS)
    # Template either uses {facts} (plural) or {fact} (single)
    if "{fact}" in template and "{facts}" in template:
        facts = rng.sample(MEM_FACTS_POOL, k=rng.randint(2, 4))
        fact = rng.choice([f for f in MEM_FACTS_POOL if f not in facts])
        return template.format(topic=topic, facts="\n- " + "\n- ".join(facts), fact=fact)
    elif "{facts}" in template:
        facts = rng.sample(MEM_FACTS_POOL, k=rng.randint(2, 4))
        return template.format(topic=topic, facts="\n- " + "\n- ".join(facts))
    else:
        return template.format(topic=topic)


def gen_general(rng: random.Random) -> str:
    template = rng.choice(GEN_TEMPLATES)
    if "{phenomenon}" in template:
        return template.format(phenomenon=rng.choice(GEN_PHENOMENA))
    if "{a}" in template and "{b}" in template:
        a, b = rng.choice(GEN_COMPARE_PAIRS)
        return template.format(a=a, b=b)
    if "{constraints}" in template:
        cs = "; ".join(rng.sample([
            "no new dependencies", "must run on AMD ROCm",
            "sub-100-ms p99 latency", "no downtime",
            "must work with existing Postgres schema",
            "code must be < 200 LoC", "must be unit-testable",
        ], k=3))
        return template.format(constraints=cs, goal=rng.choice(GEN_GOALS))
    if "{claim}" in template:
        return template.format(claim=rng.choice(GEN_PHENOMENA))
    if "{premise}" in template:
        return template.format(
            premise="saga-350m's tokenizer matches review-1.2b's tokenizer",
            question="the compatibility of their K/V projections",
        )
    if "{goal}" in template and "{context}" in template:
        return template.format(
            goal=rng.choice(GEN_GOALS),
            context="you only have access to an AMD iGPU with 2 GiB VRAM",
        )
    if "{system}" in template:
        return template.format(system=rng.choice([
            "a paged-attention KV cache", "a dual-tier embedding store",
            "an auto-restart path watcher", "a vault-backed secret resolver",
        ]))
    if "{option_a}" in template:
        a, b = rng.choice(GEN_COMPARE_PAIRS)
        cond = rng.choice([
            "throughput is more important than latency",
            "memory is tight",
            "the corpus grows daily",
            "requests have high prefix overlap",
        ])
        return template.format(option_a=a, option_b=b, condition=cond)
    return template  # fallback (shouldn't hit)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--output", required=True, help="Output JSONL path")
    ap.add_argument("--count", type=int, default=3000, help="Total prompts to generate")
    ap.add_argument("--ratio-code", type=float, default=0.40)
    ap.add_argument("--ratio-mem", type=float, default=0.30)
    # general = 1 - code - mem
    args = ap.parse_args()

    rng = random.Random(SEED)
    n_code = int(args.count * args.ratio_code)
    n_mem = int(args.count * args.ratio_mem)
    n_gen = args.count - n_code - n_mem

    prompts = []
    for _ in range(n_code):
        prompts.append({"prompt": gen_code_swarm(rng), "source": "code-swarm"})
    for _ in range(n_mem):
        prompts.append({"prompt": gen_memory(rng), "source": "mem"})
    for _ in range(n_gen):
        prompts.append({"prompt": gen_general(rng), "source": "general"})

    # Shuffle so extraction doesn't see the domains in blocks
    rng.shuffle(prompts)

    # Deduplicate on exact prompt text
    seen = set()
    unique = []
    for p in prompts:
        if p["prompt"] in seen:
            continue
        seen.add(p["prompt"])
        # Rough token estimate (4 chars ≈ 1 token)
        p["tokens_est"] = max(1, len(p["prompt"]) // 4)
        unique.append(p)

    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w") as f:
        for p in unique:
            f.write(json.dumps(p) + "\n")

    print(f"Wrote {len(unique)} unique prompts to {out}")
    print(f"  code-swarm: {sum(1 for p in unique if p['source']=='code-swarm')}")
    print(f"  mem:        {sum(1 for p in unique if p['source']=='mem')}")
    print(f"  general:    {sum(1 for p in unique if p['source']=='general')}")


if __name__ == "__main__":
    main()
