/**
 * Per-backend probes for the Models TreeView live-status enrichment
 * (Sprint 068 Phase 5).
 *
 * The extension already knows the catalog from Odin's aggregated
 * `GET /v1/models`, which gives us loaded-vs-ready per model. These
 * probes add two dimensions Odin alone cannot report:
 *
 *   1. `loaded` with VRAM bytes — direct `GET /api/ps` against each
 *      Ollama backend. Odin's aggregation aliases `loaded: bool`; the
 *      direct probe adds VRAM size so we can annotate `loaded · 8.1 GB`.
 *   2. `busy` — active user-facing chat count per backend via
 *      `GET /api/backends/busy` on Odin (Phase 6a endpoint).
 *   3. `dreaming` — `GET /status` on ygg-dreamer (Phase 6b endpoint).
 *
 * All probes fail closed (return null / empty) with a 2s timeout so a
 * single unresponsive backend never blocks a tree refresh.
 */

import * as http from "node:http";
import * as https from "node:https";

const PROBE_TIMEOUT_MS = 2000;

export interface OllamaRunningModel {
  /** Full tag, e.g. "qwen3-coder:30b-a3b-q4_K_M". */
  name: string;
  /** Bytes resident in VRAM — 0 on CPU-only loads. */
  size_vram: number;
}

export interface DreamerStatus {
  status: string;
  service?: string;
  idle_secs?: number;
  warmup_fires?: number;
  dream_fires?: number;
  active: boolean;
  active_flow?: string;
  last_fire_ts?: number;
}

/**
 * Fetch the list of models currently LOADED (VRAM-resident) on an Ollama
 * backend. Also works against llama-server / vLLM — those return 404 on
 * /api/ps, which we catch and return empty. Callers should fall back to
 * Odin's aggregated `loaded` flag in that case.
 */
export async function probeOllamaPs(
  backendUrl: string,
): Promise<Map<string, OllamaRunningModel> | null> {
  const raw = await getJson<{ models?: OllamaRunningModel[] }>(`${backendUrl}/api/ps`);
  if (!raw) return null;
  const map = new Map<string, OllamaRunningModel>();
  for (const m of raw.models ?? []) {
    if (typeof m.name === "string") {
      map.set(m.name, { name: m.name, size_vram: Number(m.size_vram ?? 0) });
    }
  }
  return map;
}

/**
 * Odin's in-flight user-facing chat counter, added server-side in
 * Sprint 068 Phase 6a. Pre-Phase-6 Odin builds return 404; we treat that
 * as "unknown" (empty Record) so the tree just omits the `busy` badge
 * rather than showing stale counts.
 */
export async function probeBusy(odinUrl: string): Promise<Record<string, number>> {
  const raw = await getJson<Record<string, number>>(`${odinUrl}/api/backends/busy`);
  return raw ?? {};
}

/**
 * Poll ygg-dreamer's `/status` endpoint. Phase 6b adds this; pre-6b
 * dreamer builds expose `/health` only, which is a different JSON shape
 * — if we get an unexpected payload we return `{ active: false }` so
 * the tree can't mis-label a backend as dreaming.
 */
export async function probeDreamer(dreamerUrl: string): Promise<DreamerStatus | null> {
  const raw = await getJson<DreamerStatus>(`${dreamerUrl}/status`);
  if (!raw || typeof raw !== "object") return null;
  if (typeof raw.active !== "boolean") {
    // Fallback — treat missing-key as not-active.
    return { status: raw.status ?? "unknown", active: false };
  }
  return raw;
}

// ── internal ─────────────────────────────────────────────────────────

function getJson<T>(urlStr: string): Promise<T | null> {
  return new Promise((resolve) => {
    let url: URL;
    try {
      url = new URL(urlStr);
    } catch {
      resolve(null);
      return;
    }
    const client = url.protocol === "https:" ? https : http;
    const timer = setTimeout(() => {
      req.destroy(new Error("probe timeout"));
      resolve(null);
    }, PROBE_TIMEOUT_MS);
    const req = client.request(
      {
        method: "GET",
        hostname: url.hostname,
        port: url.port || (url.protocol === "https:" ? 443 : 80),
        path: url.pathname + url.search,
        headers: { Accept: "application/json" },
        timeout: PROBE_TIMEOUT_MS,
      },
      (res) => {
        if (!res.statusCode || res.statusCode >= 400) {
          clearTimeout(timer);
          res.resume();
          resolve(null);
          return;
        }
        let buf = "";
        res.setEncoding("utf-8");
        res.on("data", (chunk: string) => {
          buf += chunk;
        });
        res.on("end", () => {
          clearTimeout(timer);
          try {
            resolve(JSON.parse(buf) as T);
          } catch {
            resolve(null);
          }
        });
        res.on("error", () => {
          clearTimeout(timer);
          resolve(null);
        });
      },
    );
    req.on("error", () => {
      clearTimeout(timer);
      resolve(null);
    });
    req.end();
  });
}
