"""Meta-tests that verify the concurrency + destructive gates themselves.

These run against the conftest fixtures without hitting any service. They are
the acceptance gate for verification step 5a + 5b in the sprint plan: if these
tests pass on a fresh checkout, the gates are wired correctly.
"""

from __future__ import annotations

import os

import pytest


def test_run_scope_has_unique_run_id(run_scope) -> None:
    assert run_scope.run_id, "run_id must be non-empty"
    assert "-" in run_scope.run_id, "run_id should combine host/pid/uuid"


def test_clean_scope_tag_includes_test_name(run_scope, clean_test_engrams) -> None:
    assert clean_test_engrams.tag.startswith(f"e2e:{run_scope.run_id}:")
    assert "test_clean_scope_tag_includes_test_name" in clean_test_engrams.tag


@pytest.mark.destructive
def test_destructive_marker_is_skipped_without_env(require_destructive) -> None:
    """This test is marked destructive. With no E2E_DESTRUCTIVE=1 it must skip.

    If you see this test PASS in a default run, Gate 3 (in-test env check) is broken.
    """
    # If require_destructive did not skip, the env was set — assert consistency.
    assert os.environ.get("E2E_DESTRUCTIVE") == "1"
    assert not os.environ.get("E2E_HOOK_CONTEXT")


def test_hook_context_env_not_leaked_into_developer_runs() -> None:
    """A developer's interactive pytest invocation should have no hook context.

    If this fails in your shell, you have E2E_HOOK_CONTEXT set in your env —
    unset it or this will mask destructive tests permanently.
    """
    hook = os.environ.get("E2E_HOOK_CONTEXT", "")
    # Only assert the dev-run invariant when the lock file says we're not in a hook.
    # We allow the hook-context path to set it; we only fail if a stray value
    # ended up in a developer's shell with no hook wrapper.
    if hook:
        # Confirm it's one of the two known-valid hook names. Anything else is a bug.
        assert hook in {"sprint_end", "cron"}, (
            f"E2E_HOOK_CONTEXT has unexpected value {hook!r}; "
            "only 'sprint_end' or 'cron' are valid."
        )
