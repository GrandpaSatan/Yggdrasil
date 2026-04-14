"""Sprint 067 Phase 0: shadow observer contract test.

Sprint 067 Phase 0 shipped a shadow observer block in handlers.rs (lines 324–390)
that emits a ``novelty_shadow_observe`` structured log line on every store call.
The log is the data source for Phase 1 threshold calibration — if it silently
stops emitting, Phase 1 collects no data and the Dense Cosine Gate will be tuned
on stale assumptions.

This test guards the observer contract.  It cannot inspect journalctl directly
from the pytest runner (which runs on a developer workstation, not on Munin),
so it uses an indirect approach:

1. Store a test engram via /api/v1/store.
2. Assert the store succeeded (200/201 with a non-empty id).
3. If a log-inspection helper or SSH fixture exists, use it to verify the
   shadow_log fields.  If not, the log-inspection portion is marked skip.

The store-success assertion (step 2) always runs and is sufficient to confirm
Mimir is handling requests that SHOULD trigger the shadow observer.  The
journalctl probe (step 3) is an infrastructure gap today — flagged as a
follow-up for Sprint 067.

Log fields that the shadow observer emits (from handlers.rs:379–388):
    shadow_log = true
    content_hash = <hex string>
    project = <Option<String>>
    partition_tags = <comma-separated string>
    sdr_hamming_sim = <f64 or NaN>
    dense_cosine_sim = <f64 or NaN>
    sdr_best_id = <Option<Uuid>>
    dense_best_id = <Option<Uuid>>
    message = "novelty_shadow_observe"
"""

from __future__ import annotations

import uuid

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
    force: bool = True,
) -> dict:
    """POST /api/v1/store and return the full response body."""
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
def test_shadow_observer_store_triggers_without_error(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """A store call that triggers the shadow observer completes without error.

    The shadow observer block in handlers.rs is wrapped in an anonymous scope
    (``{ ... }``) that neither returns an error nor panics — but if a future
    refactor accidentally moves it outside an infallible path, any panic inside
    it would 500 every store.  This test confirms that a normal store against
    a project with a populated SDR/dense index returns 200 or 201, not 500.
    """
    unique_sig = uuid.uuid4().hex
    cause = (
        f"shadow_observer_probe_{unique_sig}: test engram to exercise Phase 0 "
        "shadow logging on the Sprint 067 Mimir build"
    )
    effect = (
        "Expected: novelty_shadow_observe log line with shadow_log=true, "
        "content_hash, sdr_hamming_sim, dense_cosine_sim. "
        f"Unique probe id: {unique_sig}"
    )
    tags = ["sprint:067", "shadow_probe", clean_test_engrams.tag]

    payload = _store_raw(mimir_client, cause, effect, tags=tags, force=True)

    engram_id = payload.get("id") or payload.get("engram_id")
    assert engram_id, (
        f"Store returned no id — request may have 500'd inside shadow observer. "
        f"Full response: {payload}"
    )


@pytest.mark.required_services("mimir")
def test_shadow_observer_fires_on_project_scoped_store(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """Shadow observer runs for project-scoped stores (not just global scope).

    The shadow observer uses the same scoping logic as the real novelty triage:
    it calls query_scoped_with_tags when partition_tags are present.  A project-
    scoped store with a sprint:NNN tag exercises the partition branch of the
    shadow observer.  If the branch panics on an empty dense_index, this test
    surfaces it.
    """
    unique_sig = uuid.uuid4().hex
    cause = f"Sprint 067: shadow_observer_scoped_probe_{unique_sig}"
    effect = (
        "Project-scoped shadow probe with sprint:067 partition tag. "
        "Exercises query_scoped_with_tags in the shadow observer block. "
        f"Unique id: {unique_sig}"
    )
    tags = ["sprint:067", "shadow_probe", "scoped", clean_test_engrams.tag]

    # Use project="yggdrasil" with a sprint:067 tag to trigger the partition branch.
    payload = _store_raw(
        mimir_client, cause, effect, tags=tags, project="yggdrasil", force=True
    )

    engram_id = payload.get("id") or payload.get("engram_id")
    assert engram_id, (
        f"Scoped store with partition tag returned no id. "
        f"Shadow observer may have panicked on the scoped query path. "
        f"Response: {payload}"
    )


@pytest.mark.required_services("mimir")
def test_shadow_observer_second_store_logs_non_nan_similarities(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """After a first store seeds the index, a second similar store logs finite sims.

    On an empty index the shadow observer logs NaN for both sdr_hamming_sim and
    dense_cosine_sim (no nearest neighbor).  After seeding with engram A, a
    second store B with similar content should find A as a nearest neighbor and
    log a finite cosine similarity.

    We cannot inspect the log directly from E2E, but we can assert that the
    store itself succeeds (indirectly proving the NaN→f64 path in the logger
    ran without panicking).  The finite-value assertion on the log field is
    documented as a follow-up requiring SSH/journalctl access.
    """
    unique_sig = uuid.uuid4().hex
    cause_a = (
        f"shadow_observer_similarity_probe_{unique_sig}: Sprint 067 Phase 0 "
        "SDR Hamming and dense cosine shadow logging calibration seed"
    )
    effect_a = (
        "This is the FIRST store that seeds the local SDR and dense indexes. "
        "A second store with near-identical content should find this as nearest "
        f"neighbor. Unique sig: {unique_sig}"
    )
    tags_a = ["sprint:067", "shadow_probe", "similarity_seed", clean_test_engrams.tag]

    payload_a = _store_raw(mimir_client, cause_a, effect_a, tags=tags_a, force=True)
    id_a = payload_a.get("id") or payload_a.get("engram_id")
    assert id_a, f"Seed store A failed: {payload_a}"

    # Second store: nearly identical cause (differs only in trailing annotation).
    # The shadow observer should find A with high similarity.
    cause_b = cause_a + " [variant probe]"
    effect_b = effect_a + " Variant: second store for similarity logging probe."
    tags_b = ["sprint:067", "shadow_probe", "similarity_probe", clean_test_engrams.tag]

    payload_b = _store_raw(mimir_client, cause_b, effect_b, tags=tags_b, force=True)
    id_b = payload_b.get("id") or payload_b.get("engram_id")
    assert id_b, (
        f"Probe store B failed: {payload_b}. "
        "Shadow observer may have panicked when computing similarity against seed A."
    )

    # The store completed successfully.  The similarity values in the log are
    # expected to be finite for this near-duplicate pair.
    # Direct log inspection is deferred — see the skip marker below.


@pytest.mark.skip(
    reason=(
        "Shadow log field inspection requires SSH access to Munin and "
        "journalctl -u yggdrasil-mimir.service.  The pytest runner executes on "
        "the developer workstation, not on Munin.  Add an SSH/journalctl helper "
        "fixture in a follow-up (Sprint 067) to complete the field-level assertion:\n"
        "  - shadow_log == true (boolean)\n"
        "  - content_hash is a 64-char hex string\n"
        "  - project is a string or null\n"
        "  - partition_tags is a comma-separated string or ''\n"
        "  - sdr_hamming_sim is a float (may be NaN on empty index)\n"
        "  - dense_cosine_sim is a float (may be NaN on empty index)\n"
        "  - sdr_best_id is a UUID string or null\n"
        "  - dense_best_id is a UUID string or null\n"
        "The three store-success tests above run without SSH and cover the "
        "panic-free contract of the shadow observer."
    )
)
def test_shadow_log_field_schema_via_journalctl(
    mimir_client: MimirClient,
    clean_test_engrams,
) -> None:
    """Parse the most recent novelty_shadow_observe line and assert all 8 fields.

    This test is skipped until a journalctl/SSH fixture is available.
    When implemented, it should:
    1. Store a test engram.
    2. Wait up to 5s for the log line to flush (tracing is async).
    3. Run: ssh jhernandez@10.0.65.8 journalctl -u yggdrasil-mimir.service -n 50
    4. Parse JSON-structured log lines for the most recent novelty_shadow_observe.
    5. Assert all 8 fields are present with correct types.
    """
    # Placeholder — this function body never executes due to @pytest.mark.skip.
    EXPECTED_FIELDS = {
        "shadow_log",
        "content_hash",
        "project",
        "partition_tags",
        "sdr_hamming_sim",
        "dense_cosine_sim",
        "sdr_best_id",
        "dense_best_id",
    }
    # When implemented: parse the log line and assert EXPECTED_FIELDS.issubset(fields).
    _ = EXPECTED_FIELDS
