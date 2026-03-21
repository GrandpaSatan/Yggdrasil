"""Kokoro TTS Server — FastAPI wrapper for kokoro-onnx"""

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

class TTSRequest(BaseModel):
    text: str
    voice: str = "af_heart"
    speed: float = 1.0

@app.on_event("startup")
async def load_model():
    global kokoro
    from kokoro_onnx import Kokoro
    log.info("loading Kokoro ONNX...")
    kokoro = Kokoro("kokoro-v1.0.onnx", "voices-v1.0.bin")
    log.info("Kokoro loaded")

@app.get("/health")
async def health():
    return {"status": "ok", "model": "kokoro-v1.0"}

@app.post("/api/v1/tts")
async def tts(req: TTSRequest):
    t0 = time.time()
    audio, sr = kokoro.create(req.text, voice=req.voice, speed=req.speed)
    # Convert float32 → int16 PCM (what Odin + browser client expect).
    pcm_int16 = (np.clip(audio, -1.0, 1.0) * 32767).astype(np.int16)
    elapsed = time.time() - t0
    log.info("synthesized in %.2fs (%d samples, %dHz): %s", elapsed, len(pcm_int16), sr, req.text[:60])
    return Response(
        content=pcm_int16.tobytes(),
        media_type="application/octet-stream",
        headers={"x-sample-rate": str(sr)},
    )

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=9095)
