"""Kokoro TTS Server — FastAPI wrapper with NPU/CPU dual-backend.

Backend priority (controlled by TTS_DEVICE env var):
  1. NPU  — OpenVINO EP on Intel AI Boost (default)
  2. CPU  — default ONNX Runtime CPUExecutionProvider (fallback, or TTS_DEVICE=CPU)
"""

import os
import time
import logging
import numpy as np
from fastapi import FastAPI
from fastapi.responses import Response
from pydantic import BaseModel

log = logging.getLogger("tts")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

app = FastAPI(title="Kokoro TTS")

kokoro = None
backend_name: str = "none"


class TTSRequest(BaseModel):
    text: str
    voice: str = "af_heart"
    speed: float = 1.0


@app.on_event("startup")
async def load_model():
    global kokoro, backend_name
    from kokoro_onnx import Kokoro
    import onnxruntime as ort

    device_pref = os.environ.get("TTS_DEVICE", "NPU").upper()
    log.info("TTS_DEVICE=%s, loading Kokoro...", device_pref)

    kokoro = Kokoro("kokoro-v1.0.onnx", "voices-v1.0.bin")

    if device_pref == "CPU":
        backend_name = "cpu"
        log.info("Backend: CPU (forced via TTS_DEVICE=CPU)")
        return

    # Try replacing the session with NPU-accelerated one
    try:
        npu_sess = ort.InferenceSession(
            "kokoro-v1.0.onnx",
            providers=["OpenVINOExecutionProvider"],
            provider_options=[{"device_type": "NPU"}],
        )
        # Warmup
        dummy_tokens = np.array([[0, 1, 2]], dtype=np.int64)
        dummy_style = np.zeros((1, 256), dtype=np.float32)
        dummy_speed = np.array([1.0], dtype=np.float32)
        npu_sess.run(None, {"tokens": dummy_tokens, "style": dummy_style, "speed": dummy_speed})

        kokoro.sess = npu_sess
        backend_name = "npu"
        log.info("Backend: NPU (Intel AI Boost via OpenVINO EP)")
    except Exception as e:
        backend_name = "cpu"
        log.warning("NPU failed (%s), using CPU fallback", e)


@app.get("/health")
async def health():
    return {"status": "ok", "model": "kokoro-v1.0", "backend": backend_name}


@app.post("/api/v1/tts")
async def tts(req: TTSRequest):
    t0 = time.time()
    audio, sr = kokoro.create(req.text, voice=req.voice, speed=req.speed)
    pcm_int16 = (np.clip(audio, -1.0, 1.0) * 32767).astype(np.int16)
    elapsed = time.time() - t0
    log.info("[%s] synthesized in %.2fs (%d samples, %dHz): %s", backend_name, elapsed, len(pcm_int16), sr, req.text[:60])
    return Response(
        content=pcm_int16.tobytes(),
        media_type="application/octet-stream",
        headers={"x-sample-rate": str(sr)},
    )


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=9095)
