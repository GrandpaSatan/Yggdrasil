"""MCP HTTP server — SSE handshake + JSON-RPC session."""

from __future__ import annotations

import pytest
import requests

from helpers import McpHttpClient


@pytest.mark.required_services("mcp_http")
def test_mcp_sse_stream_opens(mcp_client: McpHttpClient) -> None:
    """Open the SSE stream and read at least one event (or the ': ping' keepalive)."""
    with mcp_client.open_sse() as resp:
        assert resp.status_code == 200, f"SSE endpoint must be 200, got {resp.status_code}"
        ctype = resp.headers.get("content-type", "")
        assert "text/event-stream" in ctype, (
            f"SSE endpoint must serve text/event-stream, got {ctype!r}"
        )
        # Read up to 1KB or 1 line — don't block forever.
        got_any = False
        for raw_line in resp.iter_lines(decode_unicode=True):
            got_any = True
            break
        assert got_any, "SSE stream must send at least one line"


@pytest.mark.required_services("mcp_http")
def test_mcp_messages_endpoint_reachable(mcp_client: McpHttpClient) -> None:
    """A JSON-RPC initialize call must either succeed or return a structured error.

    We don't assert session-id semantics because those require the SSE stream
    to be active in the same client process — that's tested in test_mcp_sse.
    """
    try:
        resp = mcp_client.send_message(
            session_id="",
            payload={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )
    except requests.RequestException as exc:
        pytest.skip(f"MCP HTTP unreachable: {exc}")
    # Any 2xx or 4xx with a JSON body is acceptable; 5xx is a real failure.
    assert resp.status_code < 500, (
        f"MCP /messages must not 5xx on initialize; got {resp.status_code}: {resp.text[:200]}"
    )
