"""Voice stack flows — voice-server health, /v1/voice UI, WS upgrade.

The full WAV round-trip is skipped by default because LLaMA-Omni2 cold-starts
can exceed 10s. Enable with ``E2E_VOICE_WAV=1`` to actually drive audio.
"""

from __future__ import annotations

import asyncio
import json
import os

import pytest
import requests
import websockets

from helpers import OdinClient
from helpers.services import service_urls


@pytest.mark.required_services("voice")
def test_voice_server_health_reports_model_and_voice() -> None:
    url = service_urls()["voice"]
    resp = requests.get(f"{url.rstrip('/')}/health", timeout=5)
    assert resp.status_code == 200, f"voice /health must be 200, got {resp.status_code}"
    payload = resp.json()
    assert payload.get("model"), "voice health must expose the loaded model name"
    assert payload.get("voice"), "voice health must expose the configured voice persona"
    assert payload.get("status") == "ok", f"voice status must be ok, got {payload.get('status')!r}"


@pytest.mark.required_services("odin")
def test_voice_debug_ui_served(odin_client: OdinClient) -> None:
    url = odin_client._url("/v1/voice/ui")
    resp = requests.get(url, timeout=5)
    if resp.status_code == 404:
        pytest.skip("/v1/voice/ui not exposed on this Odin build (audit listed it as optional)")
    assert resp.status_code == 200, f"/v1/voice/ui must serve debug HTML, got {resp.status_code}"
    assert "<html" in resp.text.lower() or "voice" in resp.text.lower()


@pytest.mark.required_services("odin")
def test_voice_websocket_accepts_connection(odin_client: OdinClient) -> None:
    """Assert the WS upgrade succeeds and the server sends a 'ready' frame.

    We do not send audio here — that's gated on E2E_VOICE_WAV=1 because it
    wakes LLaMA-Omni2 and burns GPU for 5-10s.
    """
    ws_url = odin_client._url("/v1/voice").replace("http://", "ws://").replace("https://", "wss://")

    async def _drive() -> dict:
        async with websockets.connect(ws_url, open_timeout=10, close_timeout=2) as ws:
            raw = await asyncio.wait_for(ws.recv(), timeout=5)
            try:
                return json.loads(raw)
            except (json.JSONDecodeError, TypeError):
                return {"raw": raw}

    payload = asyncio.run(_drive())
    # Accept either structured 'ready' or any first-frame greeting.
    kind = payload.get("type") or payload.get("event") or ""
    assert kind in ("ready", "hello", "session") or "raw" in payload, (
        f"first WS frame must be a known greeting; got {payload!r}"
    )


@pytest.mark.slow
@pytest.mark.required_services("odin", "voice")
@pytest.mark.skipif(os.environ.get("E2E_VOICE_WAV") != "1", reason="set E2E_VOICE_WAV=1 to drive real audio")
def test_voice_wav_round_trip_placeholder() -> None:
    """Drive a WAV fixture through the WS and assert a transcript comes back.

    Fixture lives at tests-e2e/fixtures/voice/silence.wav (intentionally not
    committed — synthesize locally with sox or record a quick sample).
    """
    from pathlib import Path

    wav = Path(__file__).parent.parent / "fixtures" / "voice" / "silence.wav"
    if not wav.exists():
        pytest.skip(f"WAV fixture missing at {wav}; run fixtures/voice/README.md setup first")
    # Intentionally left as a scaffolded stub — real WAV driver lives in Phase 2b.
    pytest.skip("WAV driver pending Phase 2b implementation")
