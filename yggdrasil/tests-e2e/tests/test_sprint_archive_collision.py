"""Regression test: sprint-archive collision (Sprint 065 A·P1 bug).

Background
----------
Sprint 064 P1.5 discovered that storing consecutive sprint-archive summaries
("Sprint 065: archived", "Sprint 066: archived", …) caused engrams to overwrite
each other.  The root cause: SDR binarization discards 128 of 384 embedding
dimensions, leaving the compact 256-bit SDR nearly identical for cause strings
that differ only in a 3-digit sprint number (the embedding dimensions that
encode the numeral collapse to the same bits).  With SDR similarity ~1.0 and
the store_gate LLM seeing two sprint summaries with overlapping vocabulary
(both list backend changes, deployments, gotchas), it classified the second as
an Update of the first — overwriting the Sprint 065 engram UUID with Sprint 066
content.

Sprint 065 A·P1 shipped tag-partitioned SDR lookup: engrams whose tag set
contains ``sprint:NNN`` are partitioned so sprint:065 engrams never match
sprint:066 engrams, regardless of SDR similarity.  The post-deploy smoke test
confirmed distinct UUIDs a6c966b5 and 0024fb06 for sprint:991 vs sprint:992
(memory engram verified, see ``cf6fe76c``).

This test is the PERMANENT regression guard.  It must:
  - PASS today (Sprint 065 partition is live).
  - Continue to PASS after Phase 2 rewires the Dense Cosine Gate.
  - FAIL loudly if the partition logic is accidentally removed or bypassed.

If this test fails, stop the sprint — the Sprint 065 regression has recurred.
"""

from __future__ import annotations

import pytest
import requests

from helpers import MimirClient


# ──────────────────────────────────────────────────────────────────────────────
# Helpers
# ──────────────────────────────────────────────────────────────────────────────

def _store_raw(
    mimir_client: MimirClient,
    cause: str,
    effect: str,
    *,
    tags: list[str],
    project: str = "yggdrasil",
    force: bool = False,
) -> dict:
    """POST /api/v1/store and return the full response payload including verdict."""
    body = {
        "cause": cause,
        "effect": effect,
        "tags": tags,
        "project": project,
        "force": force,
    }
    resp = requests.post(
        mimir_client._url("/api/v1/store"),
        json=body,
        timeout=mimir_client.timeout,
    )
    resp.raise_for_status()
    return resp.json()


# ──────────────────────────────────────────────────────────────────────────────
# Tests
# ──────────────────────────────────────────────────────────────────────────────

@pytest.mark.required_services("mimir")
def test_sprint_archive_collision_distinct_uuids(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """Storing Sprint 065 and Sprint 066 archive summaries yields two distinct UUIDs.

    Both stores must receive verdict=new.  If either receives verdict=update or
    verdict=old, the Sprint 065 tag-partition fix has regressed.

    CRITICAL: if this test fails, file a P0 incident.  The sprint-archive
    overwrite bug caused data loss in Sprint 063/064 (lost UUID b30d42d8 twice).
    """
    e2e_tag = clean_test_engrams.tag

    # ── Store engram X: Sprint 065 archive ────────────────────────────────────
    payload_x = _store_raw(
        mimir_client,
        cause="Sprint 065: archived",
        effect=(
            "Sprint 065 shipped tag-partitioned SDR deployed + verified. "
            "ygg-dreamer crate shipped and active on Munin. "
            "Track B infra authored. VSIX 0.14.0 released to Gitea."
        ),
        tags=["sprint:065", "close_out_bundle", e2e_tag],
        force=False,
    )

    uuid_x = payload_x.get("id") or payload_x.get("engram_id")
    verdict_x = payload_x.get("verdict", "new")

    assert uuid_x, (
        f"Sprint 065 archive store returned no id: {payload_x}. "
        "Check that Mimir is running and /api/v1/store is reachable."
    )
    assert verdict_x == "new", (
        f"Sprint 065 archive received verdict={verdict_x!r} instead of 'new'. "
        "The tag-partition fix may have regressed. "
        f"Full response: {payload_x}"
    )

    # ── Store engram Y: Sprint 066 archive ────────────────────────────────────
    payload_y = _store_raw(
        mimir_client,
        cause="Sprint 066: archived",
        effect=(
            "Sprint 066 replaced all unit/integration tests with pytest E2E suite "
            "(62 tests across 23 files). "
            "Wired 12 strict-xfail audit gates. "
            "Removed ygg-test-harness crate and ~6,000 LOC of inline tests."
        ),
        tags=["sprint:066", "close_out_bundle", e2e_tag],
        force=False,
    )

    uuid_y = payload_y.get("id") or payload_y.get("engram_id")
    verdict_y = payload_y.get("verdict", "new")

    assert uuid_y, (
        f"Sprint 066 archive store returned no id: {payload_y}. "
        "Check that Mimir is running and /api/v1/store is reachable."
    )
    assert verdict_y == "new", (
        f"Sprint 066 archive received verdict={verdict_y!r} instead of 'new'. "
        "The tag-partition fix may have regressed. "
        f"Full response: {payload_y}"
    )

    # ── Critical: UUIDs must be distinct ─────────────────────────────────────
    assert uuid_x != uuid_y, (
        f"Sprint 065 and Sprint 066 archive engrams received the SAME UUID "
        f"({uuid_x!r}).  This is the sprint-archive collision regression. "
        "The Sprint 065 A·P1 tag-partition fix is no longer working."
    )


@pytest.mark.required_services("mimir")
def test_sprint_archive_collision_content_roundtrip(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """Engrams stored with distinct sprint tags round-trip with correct content.

    After both engrams are stored, fetch each by ID and verify the cause/effect
    text matches what was sent.  This guards against silent content corruption
    where the UUID is distinct but the stored content was mixed up by the
    handler's update-in-place logic.
    """
    e2e_tag = clean_test_engrams.tag

    cause_065 = "Sprint 065: archived"
    effect_065 = (
        "Sprint 065 roundtrip check: tag-partitioned SDR lookup, ygg-dreamer crate, "
        "Track B infra authored, VSIX 0.14.0. E2E tag: " + e2e_tag
    )
    cause_066 = "Sprint 066: archived"
    effect_066 = (
        "Sprint 066 roundtrip check: replaced in-crate tests with pytest E2E suite, "
        "12 strict-xfail audit gates, ygg-test-harness removed. E2E tag: " + e2e_tag
    )

    payload_065 = _store_raw(
        mimir_client,
        cause=cause_065,
        effect=effect_065,
        tags=["sprint:065", "close_out_bundle", "roundtrip", e2e_tag],
        force=False,
    )
    payload_066 = _store_raw(
        mimir_client,
        cause=cause_066,
        effect=effect_066,
        tags=["sprint:066", "close_out_bundle", "roundtrip", e2e_tag],
        force=False,
    )

    id_065 = payload_065.get("id") or payload_065.get("engram_id")
    id_066 = payload_066.get("id") or payload_066.get("engram_id")

    assert id_065 and id_066, (
        f"One or both stores failed: 065={payload_065}, 066={payload_066}"
    )
    assert id_065 != id_066, (
        f"Collision: both sprints got UUID {id_065!r}.  Partition fix regressed."
    )

    # Fetch by ID and verify content integrity.
    engram_065 = mimir_client.get_engram(id_065)
    engram_066 = mimir_client.get_engram(id_066)

    assert engram_065 is not None, (
        f"GET /api/v1/engrams/{id_065} returned 404 — Sprint 065 engram missing. "
        "This is a store-then-fetch regression."
    )
    assert engram_066 is not None, (
        f"GET /api/v1/engrams/{id_066} returned 404 — Sprint 066 engram missing."
    )

    # Verify cause text matches what we sent.
    stored_cause_065 = engram_065.get("cause") or engram_065.get("cause_text", "")
    stored_cause_066 = engram_066.get("cause") or engram_066.get("cause_text", "")

    assert cause_065 in stored_cause_065 or stored_cause_065 in cause_065, (
        f"Sprint 065 engram cause mismatch: sent={cause_065!r}, got={stored_cause_065!r}. "
        "Content may have been overwritten by the Sprint 066 store."
    )
    assert cause_066 in stored_cause_066 or stored_cause_066 in cause_066, (
        f"Sprint 066 engram cause mismatch: sent={cause_066!r}, got={stored_cause_066!r}. "
        "Content may have been overwritten by the Sprint 065 store."
    )


@pytest.mark.required_services("mimir")
def test_sprint_archive_collision_auto_detected_partition_tags(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """Sprint number in cause text auto-generates partition tags even without explicit tags.

    Sprint 065 A·P1 also added detect_partition_tags() in handlers.rs: it parses
    the cause text for "sprint NNN" / "sprint-NNN" / "sprint:NNN" patterns and
    injects the corresponding partition tag automatically.  This means callers that
    do NOT explicitly pass sprint:NNN in their tag list still get partitioned,
    which is the critical protection for dreamer's raw store calls.
    """
    e2e_tag = clean_test_engrams.tag

    # These stores do NOT include sprint:NNN in the tags — the handler must
    # auto-detect the partition from the cause text.
    payload_a = _store_raw(
        mimir_client,
        cause="Sprint 073: archived",
        effect=(
            "Sprint 073 auto-partition probe: this engram is stored WITHOUT an "
            "explicit sprint:073 tag. The handler should auto-detect it. "
            f"E2E run tag: {e2e_tag}"
        ),
        tags=["close_out_bundle", "auto_partition_probe", e2e_tag],
        force=False,
    )
    payload_b = _store_raw(
        mimir_client,
        cause="Sprint 074: archived",
        effect=(
            "Sprint 074 auto-partition probe: this engram is stored WITHOUT an "
            "explicit sprint:074 tag. The handler should auto-detect it. "
            f"E2E run tag: {e2e_tag}"
        ),
        tags=["close_out_bundle", "auto_partition_probe", e2e_tag],
        force=False,
    )

    id_a = payload_a.get("id") or payload_a.get("engram_id")
    id_b = payload_b.get("id") or payload_b.get("engram_id")

    assert id_a and id_b, (
        f"One or both auto-partition stores failed: a={payload_a}, b={payload_b}"
    )

    verdict_a = payload_a.get("verdict", "new")
    verdict_b = payload_b.get("verdict", "new")

    assert verdict_a == "new", (
        f"Auto-partition store A received verdict={verdict_a!r}, expected 'new'. "
        f"Response: {payload_a}"
    )
    assert verdict_b == "new", (
        f"Auto-partition store B received verdict={verdict_b!r}, expected 'new'. "
        "If the auto-detect partition failed, the Sprint 074 store matched Sprint 073. "
        f"Response: {payload_b}"
    )
    assert id_a != id_b, (
        f"Auto-partition failed: both Sprint 073 and Sprint 074 got UUID {id_a!r}. "
        "detect_partition_tags() in handlers.rs may not be firing."
    )
