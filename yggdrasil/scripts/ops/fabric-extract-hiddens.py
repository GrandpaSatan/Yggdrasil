#!/usr/bin/env python3
"""
Yggdrasil Shared Memory Fabric — Hidden-State Extraction.
Sprint 069 Phase G.5.

For each model in the fleet, runs a forward pass on every prompt in a
shared corpus and captures:
- Per-layer K tensors (post-RoPE)
- Per-layer V tensors
- Final layer hidden state
- Input token ids

Output: one .pt file per (model, prompt) pair.

Runs on Morrigan (2x RTX 3060, CUDA). Designed to be resumable —
existing .pt files are skipped on restart.

Usage:
    python3 fabric-extract-hiddens.py \
        --prompts ~/fine-tuning/fabric-data/prompts.jsonl \
        --output-dir ~/fine-tuning/fabric-data/hiddens \
        --limit 300 \
        --max-seq 256
"""

import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

# Fleet roster — BASE models only. Yggdrasil production fleet is
# LFM2/LFM2.5 + Gemma + Nemotron + GLM + RWKV. NO Qwen. See
# memory/project_model_fleet.md and memory/feedback_never_qwen.md.
#
# Loaded from Morrigan's HuggingFace cache (~/.cache/huggingface/hub/).
MODELS = {
    "lfm2-350m":        "LiquidAI/LFM2-350M",
    "lfm2.5-1.2b-base": "LiquidAI/LFM2.5-1.2B-Base",
    "gemma-4-e2b":      "google/gemma-4-E2B",            # GATED — needs HF access grant
    "gemma-4-e4b":      "google/gemma-4-E4B",            # GATED — needs HF access grant
    "nemotron-3-nano-4b": "nvidia/NVIDIA-Nemotron-3-Nano-4B-BF16",  # released Dec 2025/Jan 2026, hybrid Mamba+attn
    "rwkv-7-world":     "BlinkDL/rwkv-7-world",          # RWKV needs special extraction (no K/V)
    # glm-4.7-flash:   "THUDM/GLM-4.7-Flash"             # 30B MoE — Hugin-only, needs 4-bit quant
}


def load_prompts(path: Path, limit: int):
    prompts = []
    with path.open() as f:
        for line in f:
            prompts.append(json.loads(line))
            if len(prompts) >= limit:
                break
    return prompts


def extract_for_model(model_name, model_path, prompts, output_dir, device, max_seq):
    out_dir = Path(output_dir) / model_name
    out_dir.mkdir(parents=True, exist_ok=True)

    # Skip model entirely if all prompts already extracted
    existing = {p.stem for p in out_dir.glob("*.pt")}
    to_process = [p for p in prompts
                  if hashlib.sha256(p["prompt"].encode()).hexdigest()[:16] not in existing]
    if not to_process:
        print(f"[{model_name}] all {len(prompts)} prompts already extracted, skipping")
        return
    print(f"[{model_name}] {len(to_process)}/{len(prompts)} remaining")

    print(f"[{model_name}] loading {model_path}")
    t0 = time.time()
    tokenizer = AutoTokenizer.from_pretrained(model_path, trust_remote_code=True)

    # Large multimodal Gemma-4 E4B and Nemotron-3-Nano-4B models don't fit in
    # a single RTX 3060 at fp16 (~12 GB VRAM budget). Opt into bitsandbytes
    # 4-bit quant when the env flag is set. Hidden states produced from 4-bit-
    # loaded weights have small dequantization noise but preserve the
    # residual-stream geometry that matters for projection training.
    load_kwargs = dict(
        trust_remote_code=True,
        device_map={"": device},
    )
    if os.environ.get("YGG_EXTRACT_4BIT") == "1":
        from transformers import BitsAndBytesConfig
        load_kwargs["quantization_config"] = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_compute_dtype=torch.float16,
            bnb_4bit_quant_type="nf4",
        )
        print(f"[{model_name}] 4-bit quantized load (bnb nf4)")
    else:
        load_kwargs["dtype"] = torch.float16

    model = AutoModelForCausalLM.from_pretrained(model_path, **load_kwargs)
    model.eval()
    print(f"[{model_name}] loaded in {time.time()-t0:.1f}s")

    # Architecture-specific cache adapter. NemotronH (and possibly other
    # Mamba-hybrid archs) needs a pre-allocated HybridDynamicCache and
    # exposes K/V via `outputs.cache_params` rather than past_key_values.
    is_nemotron_h = "NemotronH" in type(model).__name__
    if is_nemotron_h:
        # Nemotron-H ships its hybrid cache via trust_remote_code inside the
        # same module as the model class.
        import sys
        mod = sys.modules.get(type(model).__module__)
        nh_cache_cls = getattr(mod, "HybridMambaAttentionDynamicCache", None) if mod else None
        if nh_cache_cls is None:
            raise RuntimeError(
                f"HybridMambaAttentionDynamicCache not found in {type(model).__module__}"
            )
        print(f"[{model_name}] NemotronH adapter active, cache class {nh_cache_cls.__name__}")

    # Universal attention K/V capture via forward hooks. Works regardless of
    # whether the model populates its cache during prefill. Hooks register on
    # every k_proj / v_proj Linear inside attention modules.
    attention_kv_capture = {}  # layer_idx -> {"K": tensor, "V": tensor}
    attention_kv_hooks = []

    def _find_layer_idx(name):
        # Common naming: "model.layers.12.self_attn.k_proj" → 12
        import re
        m = re.search(r"layers?\.(\d+)\.", name)
        return int(m.group(1)) if m else -1

    def _make_kv_hook(layer_idx, kind):
        def hook(module, input, output):
            # output shape: (batch, seq, heads*head_dim) from k_proj/v_proj Linear
            if layer_idx not in attention_kv_capture:
                attention_kv_capture[layer_idx] = {}
            attention_kv_capture[layer_idx][kind] = output.detach()
        return hook

    for name, module in model.named_modules():
        # Hook every .k_proj / .v_proj Linear. Only attention-like modules
        # have this pair; naming varies across families (self_attn on LFM2/Gemma,
        # mixer on NemotronH).
        if name.endswith(".k_proj") or name.endswith(".v_proj"):
            layer_idx = _find_layer_idx(name)
            if layer_idx < 0:
                continue
            kind = "K" if name.endswith(".k_proj") else "V"
            h = module.register_forward_hook(_make_kv_hook(layer_idx, kind))
            attention_kv_hooks.append(h)
    if attention_kv_hooks:
        print(f"[{model_name}] registered {len(attention_kv_hooks)} attention K/V forward hooks")

    start = time.time()
    for i, record in enumerate(to_process):
        prompt_text = record["prompt"]
        prompt_hash = hashlib.sha256(prompt_text.encode()).hexdigest()[:16]
        out_file = out_dir / f"{prompt_hash}.pt"
        if out_file.exists():
            continue

        inputs = tokenizer(prompt_text, return_tensors="pt",
                           truncation=True, max_length=max_seq).to(device)

        # Clear the hook capture dict per prompt — hooks overwrite on each
        # attention forward call.
        attention_kv_capture.clear()

        forward_kwargs = dict(
            use_cache=True,
            output_hidden_states=True,
            return_dict=True,
        )
        if is_nemotron_h:
            batch_size = inputs["input_ids"].shape[0]
            nh_cache = nh_cache_cls(
                model.config,
                batch_size=batch_size,
                dtype=next(model.parameters()).dtype,
                device=device,
            )
            forward_kwargs["past_key_values"] = nh_cache

        with torch.no_grad():
            outputs = model(**inputs, **forward_kwargs)

        # NemotronH puts its cache under `cache_params`; everyone else uses
        # `past_key_values`. If we pre-allocated the NH cache, prefer that
        # as authoritative (the module mutates it in place).
        if is_nemotron_h:
            pkv = getattr(outputs, "cache_params", None) or nh_cache
        else:
            pkv = outputs.past_key_values
        # Multimodal configs (Gemma-4, etc.) nest the text stack under
        # `text_config` — fall through to it when the top-level config
        # lacks num_hidden_layers / layer_types.
        text_cfg = getattr(model.config, "text_config", None) or model.config
        layer_types = getattr(text_cfg, "layer_types", None) or getattr(model.config, "layer_types", None)

        # Capture attention K/V for attention layers; capture conv state for
        # conv layers (LFM2 only). Pure-attention models (Qwen2.5) hit only
        # the K/V path.
        attn_layers = []   # list of (layer_idx, K, V)
        conv_layers = []   # list of (layer_idx, conv_state)

        def _cpu16(t):
            return t.detach().cpu().to(torch.float16)

        # Attribute-based access. Three supported shapes:
        #   (a) Legacy Cache (Lfm2HybridConvCache, DynamicCache ≤5.2):
        #       .key_cache / .value_cache / .conv_cache are parallel lists
        #       indexed by model-layer.
        #   (b) New Cache API (DynamicCache in transformers 5.3+ for Gemma-4
        #       etc.): .layers is a list of per-layer objects, each with
        #       .keys / .values tensors.
        #   (c) NemotronH HybridMambaAttentionDynamicCache: .conv_states +
        #       .ssm_states (Mamba recurrent state) + empty key/value_cache.
        #       Attention K/V aren't populated in prefill mode. We capture
        #       the Mamba state under a separate payload key.
        key_cache = getattr(pkv, "key_cache", None)
        value_cache = getattr(pkv, "value_cache", None)
        conv_cache = getattr(pkv, "conv_cache", None)
        conv_states_nh = getattr(pkv, "conv_states", None)  # Nemotron Mamba conv state
        ssm_states_nh = getattr(pkv, "ssm_states", None)    # Nemotron Mamba SSM state
        layer_list = getattr(pkv, "layers", None)

        # Nemotron-specific Mamba state capture.
        mamba_state = []  # [(layer_idx, conv_state, ssm_state), ...]
        if conv_states_nh is not None and ssm_states_nh is not None:
            for li in range(min(len(conv_states_nh), len(ssm_states_nh))):
                cs = conv_states_nh[li]
                ss = ssm_states_nh[li]
                if torch.is_tensor(cs) and cs.numel() > 0 and torch.is_tensor(ss) and ss.numel() > 0:
                    mamba_state.append((li, _cpu16(cs), _cpu16(ss)))

        # Fallback attention K/V from hooks — populates when cache-based
        # extraction yields empty tensors (NemotronH in prefill mode).
        hook_attn_kv = []  # [(layer_idx, K, V, "attention_hook"), ...]
        for li in sorted(attention_kv_capture.keys()):
            kv = attention_kv_capture[li]
            if "K" in kv and "V" in kv and kv["K"].numel() > 0:
                hook_attn_kv.append((li, _cpu16(kv["K"]), _cpu16(kv["V"]), "attention_hook"))

        n_layers = getattr(text_cfg, "num_hidden_layers", None) or model.config.num_hidden_layers

        if layer_list is not None and len(layer_list) > 0 and key_cache is None:
            # Path (b) — new Cache API (Gemma-4). Layer indices in the cache
            # are cache-layer indices, not model-layer indices.
            for cache_li, layer in enumerate(layer_list):
                k = getattr(layer, "keys", None)
                v = getattr(layer, "values", None)
                if torch.is_tensor(k) and torch.is_tensor(v) and k.numel() > 0:
                    lt = getattr(layer, "is_sliding", False)
                    ltype = "sliding_attention" if lt else "full_attention"
                    attn_layers.append((cache_li, _cpu16(k), _cpu16(v), ltype))
        else:
            # Path (a) — legacy parallel lists (LFM2 hybrid, Qwen DynamicCache).
            for li in range(n_layers):
                ltype = layer_types[li] if layer_types else "full_attention"
                if ltype == "conv":
                    if conv_cache is not None and li < len(conv_cache) and conv_cache[li].numel() > 0:
                        conv_layers.append((li, _cpu16(conv_cache[li])))
                else:
                    if key_cache is not None and li < len(key_cache) and key_cache[li].numel() > 0:
                        attn_layers.append((li, _cpu16(key_cache[li]), _cpu16(value_cache[li]), ltype))

        # Path (c) fallback — if cache-based extraction yielded no attention
        # K/V but the forward-hook capture did (NemotronH prefill mode),
        # promote the hooks as authoritative.
        if not attn_layers and hook_attn_kv:
            attn_layers = hook_attn_kv

        payload = {
            "prompt_hash": prompt_hash,
            "prompt": prompt_text,
            "source": record.get("source", "unknown"),
            "model_name": model_name,
            "model_path": model_path,
            "input_ids": inputs["input_ids"].cpu(),
            "attention_mask": inputs["attention_mask"].cpu(),
            "seq_len": inputs["input_ids"].shape[1],
            "layer_types": layer_types,  # None for pure-attention models
            "hidden_states": [_cpu16(h) for h in outputs.hidden_states],
            "attn_kv": [(li, k, v) for (li, k, v, _t) in attn_layers],   # backward-compat 3-tuple
            "attn_kv_typed": attn_layers,  # 4-tuple incl. layer type (full/sliding/etc)
            "conv_state": conv_layers,     # LFM2 hybrid conv state
            "mamba_state": mamba_state,    # NemotronH Mamba state [(layer, conv, ssm), ...]
        }
        torch.save(payload, out_file)

        if (i + 1) % 25 == 0 or (i + 1) == len(to_process):
            elapsed = time.time() - start
            rate = (i + 1) / max(elapsed, 0.001)
            remaining = (len(to_process) - i - 1) / max(rate, 0.001)
            print(f"[{model_name}] {i+1}/{len(to_process)} | {rate:.2f}/s | ETA {remaining/60:.1f}m | seq={payload['seq_len']}")

    # Unload to free VRAM for the next model
    del model
    torch.cuda.empty_cache()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--prompts", type=Path, required=True)
    ap.add_argument("--output-dir", type=Path, required=True)
    ap.add_argument("--models", nargs="+", default=list(MODELS.keys()),
                    help="Subset of models to extract")
    ap.add_argument("--limit", type=int, default=3000)
    ap.add_argument("--max-seq", type=int, default=256)
    ap.add_argument("--device", default="cuda:0")
    args = ap.parse_args()

    prompts = load_prompts(args.prompts, args.limit)
    print(f"Loaded {len(prompts)} prompts from {args.prompts}")
    print(f"Output dir: {args.output_dir}")
    print(f"Max seq len: {args.max_seq}")
    print(f"Device: {args.device}")
    print(f"Models: {args.models}")
    print("")

    for model_name in args.models:
        if model_name not in MODELS:
            print(f"!! unknown model {model_name}, skipping")
            continue
        model_path = MODELS[model_name]
        try:
            extract_for_model(model_name, model_path, prompts,
                              args.output_dir, args.device, args.max_seq)
        except Exception as e:
            print(f"!! [{model_name}] FAILED: {type(e).__name__}: {e}")
            import traceback
            traceback.print_exc()

    print("\nDONE")


if __name__ == "__main__":
    main()
