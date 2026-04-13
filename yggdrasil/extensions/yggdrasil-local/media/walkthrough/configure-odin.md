# Configure Odin

Odin is the router — it dispatches your requests to the right backend (e.g. `nemotron-3-nano:4b` for coding, `gemma4:e4b` for review, `glm-4.7-flash` for reasoning) and serves an OpenAI-compatible `/v1/chat/completions` endpoint that the chat panel uses.

**What to do:**
1. Click "Open Settings" below
2. In the Endpoints tab, set `Odin URL` to your server (default port: `8080`)
3. Click `Test` next to it — you should see a green check
4. Click "Save Endpoints"

If you don't have Odin running, see the [Yggdrasil setup guide](https://github.com/GrandpaSatan/Yggdrasil#quickstart).
