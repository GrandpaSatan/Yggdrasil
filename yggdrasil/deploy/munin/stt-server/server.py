"""SenseVoice STT Server — FastAPI wrapper with NPU/CPU dual-backend.

Backend priority (controlled by STT_DEVICE env var):
  1. NPU  — OpenVINO EP on Intel AI Boost (default)
  2. CPU  — OpenVINO EP on CPU (fallback, or STT_DEVICE=CPU)
  3. funasr — original PyTorch path (STT_DEVICE=FUNASR, or if ONNX unavailable)
"""

import io
import os
import re
import json
import time
import logging
import numpy as np
import soundfile as sf
from pathlib import Path
from fastapi import FastAPI, UploadFile, File, Request
from fastapi.responses import JSONResponse

log = logging.getLogger("stt")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

app = FastAPI(title="SenseVoice STT")

# Globals set at startup
backend_name: str = "none"
ort_session = None
funasr_model = None
frontend = None
tokens: list[str] = []

MODEL_DIR = Path.home() / ".cache/modelscope/hub/models/iic/SenseVoiceSmall"
ONNX_PATH = MODEL_DIR / "model_fixed.onnx"
TOKENS_PATH = MODEL_DIR / "tokens.json"

LID_DICT = {"auto": 0, "zh": 3, "en": 4, "yue": 7, "ja": 11, "ko": 12, "nospeech": 13}
TEXTNORM_DICT = {"withitn": 14, "woitn": 15}


def _load_frontend():
    """Load FunASR's WavFrontend for fbank extraction."""
    global frontend
    from funasr import AutoModel
    m = AutoModel(
        model="iic/SenseVoiceSmall",
        trust_remote_code=True,
        device="cpu",
        disable_update=True,
    )
    frontend = m.kwargs.get("frontend", None)
    if frontend is None:
        raise RuntimeError("Failed to load WavFrontend from FunASR")
    return m


def _try_ort_backend(device: str) -> bool:
    """Try creating an ONNX Runtime session with OpenVINO EP."""
    global ort_session, backend_name
    if not ONNX_PATH.exists():
        log.warning("ONNX model not found at %s", ONNX_PATH)
        return False
    try:
        import onnxruntime as ort
        sess = ort.InferenceSession(
            str(ONNX_PATH),
            providers=["OpenVINOExecutionProvider"],
            provider_options=[{"device_type": device}],
        )
        # Warmup inference
        dummy = {
            "speech": np.zeros((1, 10, 560), dtype=np.float32),
            "speech_lengths": np.array([10], dtype=np.int32),
            "language": np.array([0], dtype=np.int32),
            "textnorm": np.array([14], dtype=np.int32),
        }
        sess.run(None, dummy)
        ort_session = sess
        backend_name = f"npu" if device == "NPU" else f"cpu_ov"
        return True
    except Exception as e:
        log.warning("OpenVINO EP (%s) failed: %s", device, e)
        return False


def _extract_fbank(audio: np.ndarray, sr: int = 16000) -> tuple[np.ndarray, np.ndarray]:
    """Extract fbank features using FunASR's frontend."""
    import torch
    from funasr.frontends.wav_frontend import load_cmvn

    waveform = torch.from_numpy(audio).unsqueeze(0).float()
    lengths = torch.tensor([len(audio)], dtype=torch.long)
    feats, feat_lens = frontend(waveform, lengths)
    return feats.numpy(), feat_lens.numpy()


def _ctc_greedy_decode(logits: np.ndarray) -> str:
    """Greedy CTC decode: argmax, collapse repeats, remove blanks."""
    ids = np.argmax(logits[0], axis=-1)  # (T,)
    # Collapse consecutive duplicates
    prev = -1
    decoded = []
    for idx in ids:
        if idx != prev and idx != 0:  # 0 = blank/<unk>
            decoded.append(int(idx))
        prev = idx
    # Map to tokens
    text_parts = []
    for idx in decoded:
        if idx < len(tokens):
            text_parts.append(tokens[idx])
    text = "".join(text_parts).replace("\u2581", " ")  # SentencePiece ▁ → space
    return text.strip()


@app.on_event("startup")
async def load_model():
    global funasr_model, tokens, backend_name

    device_pref = os.environ.get("STT_DEVICE", "NPU").upper()
    log.info("STT_DEVICE=%s, loading...", device_pref)

    # Load tokens for CTC decode
    if TOKENS_PATH.exists():
        tokens = json.loads(TOKENS_PATH.read_text())
        log.info("Loaded %d tokens", len(tokens))

    # Load frontend (always needed for fbank extraction)
    funasr_model = _load_frontend()
    log.info("FunASR frontend loaded")

    if device_pref == "FUNASR":
        backend_name = "funasr"
        log.info("Using FunASR PyTorch backend (forced)")
        return

    # Try NPU first, then CPU OpenVINO
    if device_pref == "NPU" and _try_ort_backend("NPU"):
        log.info("Backend: NPU (Intel AI Boost via OpenVINO EP)")
        return
    if _try_ort_backend("CPU"):
        log.info("Backend: CPU (OpenVINO EP)")
        return

    # Final fallback: FunASR PyTorch
    backend_name = "funasr"
    log.info("Backend: FunASR PyTorch (fallback)")


@app.get("/health")
async def health():
    return {"status": "ok", "model": "SenseVoiceSmall", "backend": backend_name}


@app.post("/api/v1/stt")
async def stt(request: Request, file: UploadFile = File(None)):
    t0 = time.time()
    content_type = request.headers.get("content-type", "")
    if file is not None and "multipart" in content_type:
        audio_bytes = await file.read()
    else:
        audio_bytes = await request.body()

    # Decode audio
    try:
        audio_data, sr = sf.read(io.BytesIO(audio_bytes))
    except Exception:
        audio_data = np.frombuffer(audio_bytes, dtype=np.int16).astype(np.float32) / 32768.0
        sr = 16000

    if len(audio_data.shape) > 1:
        audio_data = audio_data.mean(axis=1)

    if backend_name == "funasr":
        # Original FunASR path
        result = funasr_model.generate(
            input=audio_data, cache={}, language="auto", use_itn=True, batch_size_s=60,
        )
        text = ""
        if result and len(result) > 0:
            text = result[0].get("text", "")
        text = re.sub(r"<\|[^|]*\|>", "", text).strip()
    else:
        # ONNX Runtime path (NPU or CPU-OV)
        feats, feat_lens = _extract_fbank(audio_data, sr)
        inputs = {
            "speech": feats,
            "speech_lengths": feat_lens.astype(np.int32),
            "language": np.array([LID_DICT["auto"]], dtype=np.int32),
            "textnorm": np.array([TEXTNORM_DICT["withitn"]], dtype=np.int32),
        }
        logits = ort_session.run(None, inputs)
        text = _ctc_greedy_decode(logits[0])
        text = re.sub(r"<\|[^|]*\|>", "", text).strip()

    elapsed = time.time() - t0
    log.info("[%s] transcribed in %.2fs: %s", backend_name, elapsed, text[:100])
    return JSONResponse({"text": text, "duration": elapsed})


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=9097)
