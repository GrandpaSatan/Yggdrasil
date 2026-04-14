"""Security audit gate harness — one xfail per VULN/FLAW finding.

Every test here is marked ``@pytest.mark.xfail(reason="VULN-NNN", strict=True)``
so that:
  - Today (vulnerability present) the test fails its assertion → XFAIL → pass.
  - After remediation the test succeeds → XPASS strict → loud failure that
    forces the maintainer to remove the xfail marker.

This is the executable form of the audit's remediation roadmap. As Phase 1, 2,
3, 4 of the roadmap land, the corresponding xfails flip and become regular
tests guarding against regression.

Findings already covered in topical test files (not duplicated here):
  - VULN-006: tests/test_mesh.py::test_mesh_forged_handshake_rejected
  - VULN-007: tests/test_webhook.py
  - VULN-008: tests/test_memory.py::test_core_tier_write_requires_admin_token
  - VULN-010/011: tests/test_memory.py::test_store_recall_unicode_roundtrip
  - FLAW-003: tests/test_memory.py::test_cross_sprint_store_isolation (now passes)

Findings that are pure algorithm / no HTTP boundary, kept as in-crate unit tests:
  - FLAW-001 (SDR drift), FLAW-002 (consolidation), FLAW-004 (token estimation),
    FLAW-007 (novelty threshold) — all under crates/mimir/src/{novelty,sdr*,store_gate}.rs

Findings outside the scope of E2E (require log scraping or destructive setup):
  - VULN-012 (RwLock poison panics): captured by stress harness in test_concurrency.py
  - VULN-015 (token logged in debug): operational concern; reviewed via tracing config
  - VULN-022 (energy_manager O(n²)): performance-tier, not a correctness bug
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest
import requests

from helpers.services import service_urls


# ──────────────────────── VULN-001: Zero auth on core services ────────────

# Endpoints that intentionally allow unauthenticated reads (health/metrics).
PUBLIC_PATHS = {"/health", "/metrics"}

# Endpoints that already enforce auth — listed so the sweep doesn't double-count.
AUTHED_PATHS = {"/api/v1/vault"}

# Service → list of (method, path, sample_body) to probe.
PROBE_MATRIX = [
    ("odin", "POST", "/v1/chat/completions", {"messages": [{"role": "user", "content": "ping"}], "stream": False}),
    ("odin", "GET", "/v1/models", None),
    ("odin", "GET", "/api/flows", None),
    ("odin", "POST", "/api/v1/webhook", {"event": "noop"}),
    ("mimir", "POST", "/api/v1/store", {"cause": "x", "effect": "y"}),
    ("mimir", "POST", "/api/v1/recall", {"text": "x", "limit": 1}),
    ("mimir", "POST", "/api/v1/timeline", {"limit": 1}),
    ("mimir", "GET", "/api/v1/stats", None),
]


@pytest.mark.xfail(
    reason="VULN-001: Odin/Mimir/Muninn have no auth middleware (vault is the only authed endpoint)",
    strict=True,
)
@pytest.mark.required_services("odin", "mimir")
def test_all_core_endpoints_reject_unauthenticated_requests() -> None:
    """Sweep every documented endpoint and assert it rejects unauthenticated calls.

    This single test is the acceptance gate for Phase 1 of the audit roadmap.
    When VULN-001 is fixed, every entry in PROBE_MATRIX must return 401/403,
    flipping this test from XFAIL to XPASS (which is a strict failure → remove
    the xfail marker, this test is now your live regression guard).
    """
    urls = service_urls()
    failures: list[str] = []
    for service_name, method, path, body in PROBE_MATRIX:
        base = urls.get(service_name, "").rstrip("/")
        if not base:
            continue
        try:
            if method == "GET":
                resp = requests.get(f"{base}{path}", timeout=5)
            else:
                resp = requests.post(f"{base}{path}", json=body, timeout=10)
        except requests.RequestException as exc:
            failures.append(f"{service_name} {method} {path} unreachable: {exc}")
            continue
        if resp.status_code not in (401, 403):
            failures.append(f"{service_name} {method} {path} returned {resp.status_code}, expected 401/403")
    assert not failures, "endpoints accepted unauthenticated requests:\n  " + "\n  ".join(failures)


# ──────────────────────── VULN-002: Plaintext sudo password ───────────────

@pytest.mark.xfail(
    reason="VULN-002: McpServerConfig.deploy_sudo_password is a plaintext String field",
    strict=True,
)
def test_deploy_sudo_password_is_vault_reference_type() -> None:
    """The field type itself must be a vault reference, not a raw String.

    Today (vulnerable): ``pub deploy_sudo_password: Option<String>``
    After fix: ``pub deploy_sudo_password: Option<VaultRef>`` or removed
    in favor of ``{{secret:DEPLOY_SUDO_PASSWORD}}`` substitution at load time.
    """
    repo_root = Path(__file__).parent.parent.parent.parent
    candidates = [
        repo_root / "yggdrasil" / "crates" / "ygg-domain" / "src" / "config.rs",
        repo_root / "yggdrasil" / "crates" / "ygg-mcp" / "src" / "local_server.rs",
    ]
    matches: list[str] = []
    for path in candidates:
        if not path.is_file():
            continue
        for line_no, line in enumerate(path.read_text().splitlines(), start=1):
            if "deploy_sudo_password" not in line:
                continue
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            # The vulnerable form is `Option<String>` (or `String`). Acceptable forms
            # mention VaultRef / SecretRef / similar wrapper types.
            if re.search(r"Option\s*<\s*String\s*>", stripped) or re.search(r":\s*String\b", stripped):
                matches.append(f"{path.relative_to(repo_root)}:{line_no} → {stripped[:120]}")
    assert not matches, (
        "deploy_sudo_password is still typed as plaintext String:\n  "
        + "\n  ".join(matches)
    )


# ──────────────────────── VULN-004: Proxmox TLS validation disabled ──────

@pytest.mark.xfail(
    reason="VULN-004: ProxmoxClient uses .danger_accept_invalid_certs(true)",
    strict=True,
)
def test_proxmox_client_does_not_disable_tls_validation() -> None:
    """grep crates/ygg-energy/src/proxmox.rs for the danger_accept_invalid_certs call.

    Passes when the call is gone or replaced by ``add_root_certificate``.
    """
    repo_root = Path(__file__).parent.parent.parent.parent
    src = repo_root / "yggdrasil" / "crates" / "ygg-energy" / "src" / "proxmox.rs"
    if not src.is_file():
        pytest.skip(f"{src} not found")
    text = src.read_text()
    # Reject the dangerous flag; allow it only inside a #[cfg(test)] block (which we don't
    # bother to detect — it's a coarse check, but accurate for the deployed binary).
    assert "danger_accept_invalid_certs(true)" not in text, (
        "ProxmoxClient still disables TLS validation (VULN-004 not remediated)"
    )


# ──────────────────────── VULN-005: HA call_service domain bypass ─────────

@pytest.mark.xfail(
    reason="VULN-005: HaClient::call_service accepts any (domain, service) pair",
    strict=True,
)
def test_ha_client_call_service_validates_domain_allowlist() -> None:
    """The HaClient::call_service signature must take an allowlist parameter.

    Static check: grep for ``fn call_service`` and require an
    ``AllowedDomains`` (or similar) parameter in the signature.
    """
    repo_root = Path(__file__).parent.parent.parent.parent
    src = repo_root / "yggdrasil" / "crates" / "ygg-ha" / "src" / "client.rs"
    if not src.is_file():
        pytest.skip(f"{src} not found")
    text = src.read_text()

    match = re.search(r"pub\s+(?:async\s+)?fn\s+call_service\s*\([^)]*\)", text, re.MULTILINE)
    assert match, "could not locate fn call_service in client.rs"
    signature = match.group(0)
    assert "AllowedDomains" in signature or "allowlist" in signature.lower() or "allow_list" in signature.lower(), (
        f"call_service signature lacks an allowlist parameter: {signature[:200]}"
    )


# ──────────────────────── VULN-013: CONTEXT_STORE unbounded growth ───────

@pytest.mark.xfail(
    reason="VULN-013: Odin session store has no TTL/LRU eviction",
    strict=True,
)
@pytest.mark.required_services("odin")
def test_odin_session_store_eviction_metric_present() -> None:
    """After eviction lands, /metrics must expose at least one ``session_evictions_total``-style counter."""
    base = service_urls()["odin"].rstrip("/")
    text = requests.get(f"{base}/metrics", timeout=5).text
    assert any(
        key in text for key in ("session_evictions_total", "context_store_evictions", "sessions_evicted")
    ), "no session-eviction counter exposed (VULN-013 unremediated?)"


# ──────────────────────── FLAW-009: Mesh gate default-allow ─────────────

@pytest.mark.xfail(
    reason="FLAW-009: GateConfig::default() returns GatePolicy::Allow",
    strict=True,
)
def test_mesh_gate_default_policy_is_deny() -> None:
    """Source-level check: the default policy must be Deny (fail-closed)."""
    repo_root = Path(__file__).parent.parent.parent.parent
    src = repo_root / "yggdrasil" / "crates" / "ygg-mesh" / "src" / "gate.rs"
    if not src.is_file():
        pytest.skip(f"{src} not found")
    text = src.read_text()

    # Look for the Default impl and assert it produces Deny.
    match = re.search(
        r"impl\s+Default\s+for\s+GateConfig\s*\{[^}]*GatePolicy::(\w+)",
        text,
        re.DOTALL,
    )
    assert match, "could not locate Default impl for GateConfig"
    policy = match.group(1)
    assert policy == "Deny", f"GateConfig default policy is {policy}, must be Deny (FLAW-009)"


# ──────────────────────── FLAW-008: Flow secrets in LLM prompt ──────────

@pytest.mark.xfail(
    reason="FLAW-008: resolved secret values are sent to the LLM in plaintext",
    strict=True,
)
def test_flow_secrets_scrubbed_from_response() -> None:
    """After remediation, an LLM response must never echo a resolved secret value.

    Today this would require a live secret + a model that echoes prompts; the
    static check here is for the existence of a post-process scrubbing function.
    """
    repo_root = Path(__file__).parent.parent.parent.parent
    src = repo_root / "yggdrasil" / "crates" / "odin" / "src" / "flow_secrets.rs"
    if not src.is_file():
        pytest.skip(f"{src} not found")
    text = src.read_text()
    assert any(
        marker in text for marker in ("scrub_response", "redact_response", "scrub_secrets_from")
    ), "flow_secrets.rs has no response-scrubbing function (FLAW-008 unremediated)"
