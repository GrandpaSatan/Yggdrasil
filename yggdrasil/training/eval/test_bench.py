#!/usr/bin/env python3
"""Yggdrasil Test Generation Benchmark.

Tests LLM ability to generate Rust test suites for real Yggdrasil functions.
Each task provides a function with context and scores the response on:
  - Structure (#[cfg(test)], mod tests, #[test] attributes)
  - Coverage (happy path + edge cases + error cases)
  - Valid Rust syntax (via rustfmt check)
  - Descriptive test names (snake_case)
  - Latency (tokens per second)

Usage:
    python test_bench.py --model LFM2.5-1.2B-Instruct --url http://localhost:11434
    python test_bench.py --model Qwen3.5-27B --backend openai --url http://${MORRIGAN_URL}

Output: JSON results file + console summary table.
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

import requests

# -----------------------------------------------------------------
# Benchmark Tasks -- generate tests for real Yggdrasil functions
# -----------------------------------------------------------------

SYSTEM_PROMPT = """You are a Rust test generator for the Yggdrasil AI homelab project.
Generate ONLY valid Rust test code. No explanations, no markdown fences, no commentary.
Follow Yggdrasil test conventions:
- Wrap in #[cfg(test)] mod tests { ... }
- Use #[test] for sync, #[tokio::test] for async
- Descriptive snake_case test names (test_<what>_<scenario>)
- Use assert!, assert_eq!, assert_ne! macros
- Cover: happy path, edge cases, error cases"""

TASKS = [
    {
        "id": "extract_json",
        "name": "Test extract_json()",
        "prompt": """Generate tests for this function from mimir/src/saga.rs:

```rust
/// Strip Qwen3 `<think>...</think>` tags and extract the first JSON object.
fn extract_json(text: &str) -> Option<String> {
    // Strip thinking tags: remove everything between <think> and </think>
    let mut cleaned = text.to_string();
    while let Some(start) = cleaned.find("<think>") {
        if let Some(end) = cleaned.find("</think>") {
            cleaned.replace_range(start..end + "</think>".len(), "");
        } else {
            // Unclosed <think> -- strip from <think> to end
            cleaned.truncate(start);
            break;
        }
    }
    let cleaned = cleaned.trim();

    // Extract first JSON object: find matching { ... }
    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')?;
    if end > start {
        Some(cleaned[start..=end].to_string())
    } else {
        None
    }
}
```

Write tests covering:
1. Plain JSON with no think tags
2. JSON wrapped in <think>...</think> tags
3. Multiple think blocks before JSON
4. Unclosed <think> tag with JSON before it
5. No JSON at all (returns None)
6. Empty input (returns None)
7. Nested JSON objects""",
        "min_tests": 5,
        "checks": {
            "has_cfg_test": r"#\[cfg\(test\)\]",
            "has_mod_tests": r"mod tests",
            "has_test_attr": r"#\[test\]",
            "has_assert": r"assert(_eq|_ne)?!",
            "has_some_check": r"Some\(",
            "has_none_check": r"None",
            "has_think_tag_test": r"<think>",
        },
    },
    {
        "id": "engram_content_hash",
        "name": "Test engram_content_hash()",
        "prompt": """Generate tests for this function from mimir/src/handlers.rs:

```rust
use sha2::{Sha256, Digest};

/// SHA-256 hash of `cause + "\\n" + effect` for engram content dedup in PG.
pub fn engram_content_hash(cause: &str, effect: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(cause.as_bytes());
    hasher.update(b"\\n");
    hasher.update(effect.as_bytes());
    hasher.finalize().to_vec()
}
```

Write tests covering:
1. Same inputs produce same hash (deterministic)
2. Different inputs produce different hashes
3. Order matters (cause/effect swapped gives different hash)
4. Empty cause and effect still produce a valid 32-byte hash
5. Unicode content hashes correctly
6. Hash length is always 32 bytes (SHA-256)""",
        "min_tests": 4,
        "checks": {
            "has_cfg_test": r"#\[cfg\(test\)\]",
            "has_mod_tests": r"mod tests",
            "has_test_attr": r"#\[test\]",
            "has_assert": r"assert(_eq|_ne)?!",
            "has_hash_len_check": r"32|len\(\)",
            "has_determinism_test": r"(same|deterministic|equal|identical)",
        },
    },
    {
        "id": "truncate_to_word_boundary",
        "name": "Test truncate_to_word_boundary()",
        "prompt": """Generate tests for this function from mimir/src/handlers.rs:

```rust
pub fn truncate_to_word_boundary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    // Find the last whitespace at or before max_chars
    let candidate = &text[..max_chars];
    if let Some(pos) = candidate.rfind(char::is_whitespace) {
        text[..pos].trim_end().to_string()
    } else {
        // No whitespace found -- hard cut
        text[..max_chars].to_string()
    }
}
```

Write tests covering:
1. Short text (under limit) returns unchanged
2. Text truncated at word boundary
3. Single long word with no spaces (hard cut)
4. Exact length match returns unchanged
5. Max_chars = 0 returns empty string
6. Text with multiple spaces truncates cleanly""",
        "min_tests": 4,
        "checks": {
            "has_cfg_test": r"#\[cfg\(test\)\]",
            "has_mod_tests": r"mod tests",
            "has_test_attr": r"#\[test\]",
            "has_assert": r"assert(_eq|_ne)?!",
            "has_boundary_test": r"(word|boundary|space|whitespace)",
            "has_short_text_test": r"(short|under|within|small|fits)",
        },
    },
    {
        "id": "build_project_filter",
        "name": "Test build_project_filter()",
        "prompt": """Generate tests for this function from mimir/src/handlers.rs:

```rust
use qdrant_client::qdrant::{Filter, Condition};

/// Build a Qdrant filter for project-scoped queries.
///
/// - project=Some + include_global: should(project=p OR scope="global")
/// - project=Some + !include_global: must(project=p)
/// - project=None: no filter (search everything)
fn build_project_filter(project: Option<&str>, include_global: bool) -> Option<Filter> {
    match project {
        Some(p) if include_global => Some(Filter::should(vec![
            Condition::matches("project", p.to_string()),
            Condition::matches("scope", "global".to_string()),
        ])),
        Some(p) => Some(Filter::must(vec![
            Condition::matches("project", p.to_string()),
        ])),
        None => None,
    }
}
```

Write tests covering:
1. None project returns None (no filter)
2. Some project with include_global=false returns must filter
3. Some project with include_global=true returns should filter
4. Verify filter contains correct project name
5. Verify global scope condition is present when include_global=true""",
        "min_tests": 3,
        "checks": {
            "has_cfg_test": r"#\[cfg\(test\)\]",
            "has_mod_tests": r"mod tests",
            "has_test_attr": r"#\[test\]",
            "has_assert": r"assert(_eq|_ne)?!|assert!",
            "has_none_case": r"None",
            "has_some_case": r"Some\(",
        },
    },
    {
        "id": "config_defaults",
        "name": "Test SagaEnrichConfig defaults",
        "prompt": """Generate tests for this config struct from ygg-domain/src/config.rs:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SagaEnrichConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_saga_url", alias = "ollama_url")]
    pub llm_url: String,
    #[serde(default = "default_saga_model")]
    pub model: String,
    #[serde(default = "default_saga_timeout")]
    pub timeout_secs: u64,
}

fn default_true() -> bool { true }
fn default_saga_url() -> String { "http://127.0.0.1:8080".to_string() }
fn default_saga_model() -> String { "LFM2.5-1.2B-Instruct".to_string() }
fn default_saga_timeout() -> u64 { 10 }
```

Write tests covering:
1. Deserialize empty JSON object -- all defaults applied
2. Explicit values override defaults
3. The "ollama_url" alias deserializes into llm_url field
4. Round-trip: serialize then deserialize produces same values
5. Default enabled is true (not false)""",
        "min_tests": 3,
        "checks": {
            "has_cfg_test": r"#\[cfg\(test\)\]",
            "has_mod_tests": r"mod tests",
            "has_test_attr": r"#\[test\]",
            "has_assert": r"assert(_eq|_ne)?!|assert!",
            "has_serde_json": r"serde_json|from_str|from_value",
            "has_default_check": r"(default|enabled.*true|127\.0\.0\.1)",
        },
    },
]


# -----------------------------------------------------------------
# Scoring
# -----------------------------------------------------------------

@dataclass
class TaskResult:
    task_id: str
    task_name: str
    model: str
    passed_checks: dict[str, bool] = field(default_factory=dict)
    syntax_valid: bool = False
    test_count: int = 0
    check_score: float = 0.0
    total_score: float = 0.0
    tokens_generated: int = 0
    latency_ms: int = 0
    tok_per_sec: float = 0.0
    response: str = ""
    error: str = ""


def check_rust_syntax(code: str) -> bool:
    """Check if code is valid Rust syntax via rustfmt."""
    # Strip markdown fences if present
    code = re.sub(r"^```\w*\n?", "", code, flags=re.MULTILINE)
    code = re.sub(r"\n?```\s*$", "", code, flags=re.MULTILINE)

    with tempfile.NamedTemporaryFile(suffix=".rs", mode="w", delete=False) as f:
        # Wrap if needed for syntax check
        if "#[cfg(test)]" not in code and "fn main" not in code:
            f.write("#![allow(unused, dead_code)]\n")
            f.write("use serde::{Serialize, Deserialize};\n\n")
        f.write(code)
        f.flush()

        try:
            result = subprocess.run(
                ["rustfmt", "--check", f.name],
                capture_output=True, text=True, timeout=10,
            )
            return result.returncode in (0, 1)
        except (subprocess.TimeoutExpired, FileNotFoundError):
            return False
        finally:
            Path(f.name).unlink(missing_ok=True)


def count_test_functions(response: str) -> int:
    """Count #[test] or #[tokio::test] attributed functions."""
    return len(re.findall(r"#\[(tokio::)?test\]", response))


def score_task(task: dict, response: str) -> tuple[dict[str, bool], float]:
    """Score a response against task checks. Returns (check_results, score)."""
    checks = task["checks"]
    results = {}

    for check_name, pattern in checks.items():
        results[check_name] = bool(re.search(pattern, response, re.DOTALL | re.IGNORECASE))

    passed = sum(1 for v in results.values() if v)
    score = passed / len(results) if results else 0.0
    return results, score


# -----------------------------------------------------------------
# Model interaction
# -----------------------------------------------------------------

def query_ollama(url: str, model: str, prompt: str, timeout: int = 120) -> tuple[str, int, float]:
    """Query Ollama API. Returns (response_text, token_count, latency_ms)."""
    start = time.monotonic()
    try:
        resp = requests.post(
            f"{url}/api/chat",
            json={
                "model": model,
                "messages": [
                    {"role": "system", "content": SYSTEM_PROMPT},
                    {"role": "user", "content": prompt},
                ],
                "stream": False,
                "options": {"temperature": 0.1, "num_predict": 2048},
            },
            timeout=timeout,
        )
        latency = (time.monotonic() - start) * 1000
        if resp.status_code != 200:
            return f"HTTP {resp.status_code}: {resp.text[:200]}", 0, latency

        data = resp.json()
        text = data.get("message", {}).get("content", "")
        tokens = data.get("eval_count", len(text.split()))
        return text, tokens, latency
    except requests.RequestException as e:
        latency = (time.monotonic() - start) * 1000
        return f"Error: {e}", 0, latency


def query_openai(url: str, model: str, prompt: str, timeout: int = 120) -> tuple[str, int, float]:
    """Query OpenAI-compatible API. Returns (response_text, token_count, latency_ms)."""
    start = time.monotonic()
    try:
        resp = requests.post(
            f"{url}/v1/chat/completions",
            json={
                "model": model,
                "messages": [
                    {"role": "system", "content": SYSTEM_PROMPT},
                    {"role": "user", "content": prompt},
                ],
                "temperature": 0.1,
                "max_tokens": 2048,
            },
            timeout=timeout,
        )
        latency = (time.monotonic() - start) * 1000
        if resp.status_code != 200:
            return f"HTTP {resp.status_code}: {resp.text[:200]}", 0, latency

        data = resp.json()
        text = data["choices"][0]["message"]["content"]
        tokens = data.get("usage", {}).get("completion_tokens", len(text.split()))
        return text, tokens, latency
    except requests.RequestException as e:
        latency = (time.monotonic() - start) * 1000
        return f"Error: {e}", 0, latency


# -----------------------------------------------------------------
# Benchmark runner
# -----------------------------------------------------------------

def run_benchmark(
    model: str,
    url: str,
    backend_type: str = "ollama",
    tasks: list[dict] | None = None,
) -> list[TaskResult]:
    """Run all tasks against a model. Returns list of TaskResult."""
    tasks = tasks or TASKS
    query_fn = query_openai if backend_type == "openai" else query_ollama
    results = []

    for task in tasks:
        print(f"  [{task['id']}] {task['name']}...", end=" ", flush=True)

        response, tokens, latency = query_fn(url, model, task["prompt"])

        if response.startswith("Error:") or response.startswith("HTTP "):
            result = TaskResult(
                task_id=task["id"],
                task_name=task["name"],
                model=model,
                error=response,
                latency_ms=int(latency),
            )
            print("ERROR")
            results.append(result)
            continue

        check_results, check_score = score_task(task, response)
        syntax_ok = check_rust_syntax(response)
        test_count = count_test_functions(response)
        tok_per_sec = (tokens / (latency / 1000)) if latency > 0 and tokens > 0 else 0

        # Scoring weights:
        # 30% check adherence (pattern checks)
        # 25% syntax validity
        # 25% test count (meets minimum)
        # 20% structure (cfg_test + mod_tests)
        syntax_score = 1.0 if syntax_ok else 0.0
        count_score = 1.0 if test_count >= task.get("min_tests", 3) else (test_count / task.get("min_tests", 3))
        structure_score = 1.0 if (check_results.get("has_cfg_test", False) and check_results.get("has_mod_tests", False)) else 0.0
        total = 0.30 * check_score + 0.25 * syntax_score + 0.25 * count_score + 0.20 * structure_score

        result = TaskResult(
            task_id=task["id"],
            task_name=task["name"],
            model=model,
            passed_checks=check_results,
            syntax_valid=syntax_ok,
            test_count=test_count,
            check_score=round(check_score, 3),
            total_score=round(total, 3),
            tokens_generated=tokens,
            latency_ms=int(latency),
            tok_per_sec=round(tok_per_sec, 1),
            response=response[:2000],
        )

        status = "PASS" if total >= 0.7 else "PARTIAL" if total >= 0.4 else "FAIL"
        print(f"{status} (score={total:.2f}, syntax={'OK' if syntax_ok else 'BAD'}, "
              f"tests={test_count}, {tokens}tok, {tok_per_sec:.1f}tok/s)")
        results.append(result)

    return results


def print_summary(all_results: dict[str, list[TaskResult]]):
    """Print comparison table across models."""
    print(f"\n{'=' * 80}")
    print("TEST GENERATION BENCHMARK RESULTS")
    print(f"{'=' * 80}")

    models = list(all_results.keys())
    tasks = TASKS

    # Header
    header = f"{'Task':<35}"
    for m in models:
        short = m.split(":")[0][-15:]
        header += f" {short:>15}"
    print(header)
    print("-" * len(header))

    # Per-task scores
    for task in tasks:
        row = f"{task['name']:<35}"
        for m in models:
            results = all_results[m]
            match = next((r for r in results if r.task_id == task["id"]), None)
            if match:
                score = f"{match.total_score:.2f} ({match.test_count}t)"
                if match.error:
                    score = "ERR"
            else:
                score = "N/A"
            row += f" {score:>15}"
        print(row)

    # Average scores
    print("-" * len(header))
    row = f"{'AVERAGE':<35}"
    for m in models:
        results = all_results[m]
        scores = [r.total_score for r in results if not r.error]
        avg = sum(scores) / len(scores) if scores else 0
        row += f" {avg:>15.3f}"
    print(row)

    # Tok/s
    row = f"{'avg tok/s':<35}"
    for m in models:
        results = all_results[m]
        rates = [r.tok_per_sec for r in results if r.tok_per_sec > 0]
        avg = sum(rates) / len(rates) if rates else 0
        row += f" {avg:>15.1f}"
    print(row)

    # Syntax pass rate
    row = f"{'syntax pass rate':<35}"
    for m in models:
        results = all_results[m]
        valid = sum(1 for r in results if r.syntax_valid and not r.error)
        total = sum(1 for r in results if not r.error)
        pct = f"{valid}/{total} ({valid/total*100:.0f}%)" if total else "N/A"
        row += f" {pct:>15}"
    print(row)
    print(f"{'=' * 80}")


# -----------------------------------------------------------------
# CLI
# -----------------------------------------------------------------

MODEL_MATRIX = [
    {"model": "hf.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF:Q4_K_M", "url": "http://localhost:11434", "backend": "ollama"},
    {"model": "hf.co/LiquidAI/LFM2-2.6B-Exp-GGUF:Q4_K_M", "url": "http://localhost:11434", "backend": "ollama"},
    {"model": "Qwen3.5-27B-Q4_K_M.gguf", "url": "http://${MORRIGAN_URL}", "backend": "openai"},
]


def main():
    parser = argparse.ArgumentParser(description="Yggdrasil Test Generation Benchmark")
    parser.add_argument("--model", help="Model name for Ollama/OpenAI API")
    parser.add_argument("--url", default="http://localhost:11434", help="API base URL")
    parser.add_argument("--backend", default="ollama", choices=["ollama", "openai"])
    parser.add_argument("--all-models", action="store_true", help="Run full model matrix")
    parser.add_argument("--output", default="test_bench_results.json", help="Output JSON file")
    parser.add_argument("--task", help="Run a single task by ID")
    args = parser.parse_args()

    if not args.model and not args.all_models:
        parser.error("Either --model or --all-models is required")

    all_results: dict[str, list[TaskResult]] = {}
    tasks = TASKS
    if args.task:
        tasks = [t for t in TASKS if t["id"] == args.task]
        if not tasks:
            parser.error(f"Unknown task: {args.task}. Available: {[t['id'] for t in TASKS]}")

    if args.all_models:
        import os
        for entry in MODEL_MATRIX:
            url = entry["url"].replace("${MORRIGAN_URL}", os.environ.get("MORRIGAN_URL", "localhost:8080"))
            model = entry["model"]
            print(f"\n{'--' * 30}")
            print(f"Model: {model}")
            print(f"URL:   {url} ({entry['backend']})")
            print(f"{'--' * 30}")
            results = run_benchmark(model, url, entry["backend"], tasks)
            all_results[model] = results
    else:
        print(f"\nModel: {args.model}")
        print(f"URL:   {args.url} ({args.backend})")
        print(f"{'--' * 30}")
        results = run_benchmark(args.model, args.url, args.backend, tasks)
        all_results[args.model] = results

    # Save results
    output_path = Path(args.output)
    serializable = {
        model: [asdict(r) for r in results]
        for model, results in all_results.items()
    }
    with open(output_path, "w") as f:
        json.dump(serializable, f, indent=2, default=str)
    print(f"\nResults saved to {output_path}")

    # Print summary
    print_summary(all_results)


if __name__ == "__main__":
    main()
