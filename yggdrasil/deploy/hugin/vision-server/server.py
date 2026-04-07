#!/usr/bin/env python3
"""LFM2.5-VL Vision Server — Hugin CPU inference.

Serves LiquidAI/LFM2.5-VL-1.6B via FastAPI with OpenAI-compatible
/v1/chat/completions endpoint for image+text understanding.

Runs on Hugin's AMD Ryzen AI 9 HX 370 CPU (fp32).
Future: migrate to NPU (AMD XDNA) when supported.

Usage:
    python server.py [--port 9096] [--model LiquidAI/LFM2.5-VL-1.6B]
"""
import argparse
import base64
import io
import time
import uuid

import torch
import uvicorn
from fastapi import FastAPI
from PIL import Image
from pydantic import BaseModel
from transformers import AutoModelForImageTextToText, AutoProcessor

app = FastAPI(title="Yggdrasil Vision Server")

model = None
processor = None
model_id = None


class ChatMessage(BaseModel):
    role: str
    content: str | list


class ChatRequest(BaseModel):
    model: str = ""
    messages: list[ChatMessage]
    max_tokens: int = 256
    temperature: float = 0.1
    stream: bool = False


def decode_image(content_item: dict) -> Image.Image | None:
    """Extract PIL Image from OpenAI-format image content."""
    if content_item.get("type") != "image_url":
        return None
    url = content_item.get("image_url", {}).get("url", "")
    if url.startswith("data:"):
        # data:image/png;base64,<data>
        b64 = url.split(",", 1)[1]
        return Image.open(io.BytesIO(base64.b64decode(b64))).convert("RGB")
    return None


@app.get("/health")
def health():
    return {"status": "ok", "model": model_id}


@app.get("/v1/models")
def list_models():
    return {"data": [{"id": model_id, "object": "model"}]}


@app.post("/v1/chat/completions")
def chat_completions(req: ChatRequest):
    images = []
    chat_messages = []

    for msg in req.messages:
        if isinstance(msg.content, str):
            chat_messages.append({"role": msg.role, "content": [{"type": "text", "text": msg.content}]})
        elif isinstance(msg.content, list):
            parts = []
            for item in msg.content:
                if isinstance(item, dict) and item.get("type") == "image_url":
                    img = decode_image(item)
                    if img:
                        images.append(img)
                        parts.append({"type": "image", "image": img})
                elif isinstance(item, dict) and item.get("type") == "text":
                    parts.append({"type": "text", "text": item.get("text", "")})
                elif isinstance(item, dict) and item.get("type") == "image":
                    # Direct image object (internal use)
                    img = item.get("image")
                    if img:
                        images.append(img)
                        parts.append({"type": "image", "image": img})
            chat_messages.append({"role": msg.role, "content": parts})

    text = processor.apply_chat_template(chat_messages, add_generation_prompt=True, tokenize=False)
    inputs = processor(text=text, images=images if images else None, return_tensors="pt")

    start = time.time()
    with torch.no_grad():
        outputs = model.generate(
            **inputs,
            max_new_tokens=req.max_tokens,
            temperature=max(req.temperature, 0.01),
            do_sample=req.temperature > 0.01,
        )
    elapsed = time.time() - start

    response_ids = outputs[0][inputs["input_ids"].shape[1]:]
    response_text = processor.decode(response_ids, skip_special_tokens=True)
    n_tokens = len(response_ids)

    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:24]}",
        "object": "chat.completion",
        "model": model_id,
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": response_text},
                "finish_reason": "stop",
            }
        ],
        "usage": {
            "prompt_tokens": inputs["input_ids"].shape[1],
            "completion_tokens": n_tokens,
            "total_tokens": inputs["input_ids"].shape[1] + n_tokens,
        },
        "timings": {
            "predicted_ms": elapsed * 1000,
            "predicted_per_token_ms": (elapsed * 1000) / max(n_tokens, 1),
            "predicted_per_second": n_tokens / max(elapsed, 0.001),
        },
    }


def main():
    global model, processor, model_id
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=9096)
    parser.add_argument("--model", default="LiquidAI/LFM2.5-VL-1.6B")
    parser.add_argument("--host", default="0.0.0.0")
    args = parser.parse_args()

    model_id = args.model
    print(f"Loading {model_id}...")
    processor = AutoProcessor.from_pretrained(model_id)
    model = AutoModelForImageTextToText.from_pretrained(
        model_id,
        dtype=torch.float32,
        device_map="cpu",
    )
    model.eval()
    print(f"Model loaded: {sum(p.numel() for p in model.parameters()) / 1e6:.1f}M params")

    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
