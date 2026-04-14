"""Shared pytest fixtures for the Yggdrasil E2E suite.

Three things that matter and are easy to break:

1. **Destructive-test gate.** A destructive test (VM launch, real SSH deploy,
   real HA actuation) must require *three* independent signals to execute.
   See ``pytest_collection_modifyitems`` and ``require_destructive`` below.

2. **Concurrency gate.** Live Mimir's store_gate queue is serial; live Odin's
   Postgres pool caps at 16. Parallel pytest workers cause 429s and DB
   contention that look like test failures. We refuse to start unless
   ``E2E_PARALLEL_OK=1`` is set, and a file lock serializes independent
   ``pytest`` processes running against the same fleet.

3. **Per-test cleanup scope.** Every engram a test creates is tagged
   ``e2e:<run_id>:<test_name>`` so teardown removes exactly what this run
   made, nothing else.
"""

from __future__ import annotations

import atexit
import os
import socket
import stat
import time
import uuid
from dataclasses import dataclass
from pathlib import Path

import pytest
from dotenv import load_dotenv

from helpers import (
    McpHttpClient,
    MimirClient,
    MuninnClient,
    OdinClient,
    ServiceHealth,
    wait_for_ready,
)
from helpers.services import probe, service_urls

LOCKFILE = Path("/tmp/yggdrasil-e2e.lock")
ENV_TEST_PATH = Path(__file__).parent / ".env.test"
HOOK_CONTEXT_ENV = "E2E_HOOK_CONTEXT"
DESTRUCTIVE_ENV = "E2E_DESTRUCTIVE"
PARALLEL_OK_ENV = "E2E_PARALLEL_OK"


# ───────────────────────── env loading + permission check ─────────────────

def _load_env_test() -> None:
    """Load .env.test if present; refuse world-readable files."""
    if not ENV_TEST_PATH.exists():
        # .env.test is optional — test may still run against defaults + real env.
        return

    st = ENV_TEST_PATH.stat()
    if st.st_mode & (stat.S_IRWXG | stat.S_IRWXO):
        raise RuntimeError(
            f"{ENV_TEST_PATH} is group- or world-readable; chmod 600 it "
            "before loading. Refusing to load credentials."
        )
    load_dotenv(dotenv_path=ENV_TEST_PATH, override=False)


_load_env_test()


# ───────────────────────── concurrency + lock management ──────────────────

def _pytest_xdist_enabled() -> bool:
    """Detect whether pytest-xdist was requested via CLI (-n N or --numprocesses)."""
    import sys

    args = sys.argv[1:]
    for i, a in enumerate(args):
        if a in ("-n", "--numprocesses"):
            return True
        if a.startswith("-n") or a.startswith("--numprocesses="):
            return True
    return False


def _acquire_lock() -> bool:
    """Best-effort exclusive lock — True if this process now owns the lockfile."""
    try:
        fd = os.open(LOCKFILE, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError:
        return False
    os.write(fd, f"{os.getpid()}\n{socket.gethostname()}\n".encode())
    os.close(fd)
    atexit.register(_release_lock)
    return True


def _release_lock() -> None:
    try:
        if LOCKFILE.exists():
            content = LOCKFILE.read_text().splitlines()
            if content and content[0].strip() == str(os.getpid()):
                LOCKFILE.unlink()
    except OSError:
        pass


# ───────────────────────── run_id + cleanup helpers ────────────────────────

@dataclass(frozen=True)
class RunScope:
    run_id: str
    sprint_id: str | None

    def tag(self, test_name: str) -> str:
        return f"e2e:{self.run_id}:{test_name}"


@dataclass
class CleanScope:
    """Per-test cleanup handle — expose ``.tag`` to tests, auto-purge on teardown."""

    tag: str
    _mimir: MimirClient
    _project: str = "yggdrasil"

    def purge(self) -> int:
        return self._mimir.delete_by_tag(self.tag, project=self._project)


# ───────────────────────── pytest session hooks ────────────────────────────

def pytest_configure(config: pytest.Config) -> None:
    """Validate concurrency gates and acquire the cross-process lock."""
    if _pytest_xdist_enabled() and os.environ.get(PARALLEL_OK_ENV) != "1":
        raise pytest.UsageError(
            "pytest-xdist is enabled but E2E_PARALLEL_OK=1 is not set. "
            "The live fleet is not tuned for parallel test workers — "
            "serial execution is the default. Set E2E_PARALLEL_OK=1 only "
            "for the test_concurrency.py stress harness."
        )

    if not _acquire_lock():
        # Another pytest process owns the fleet right now.
        content = LOCKFILE.read_text(errors="replace") if LOCKFILE.exists() else "(unknown)"
        raise pytest.UsageError(
            f"Another pytest run holds {LOCKFILE}:\n  {content}\n"
            "Wait for it to finish, or rm the file if stale."
        )

    # Record the hook context (if any) for introspection from tests.
    config.stash[pytest.StashKey[str]()] = os.environ.get(HOOK_CONTEXT_ENV, "")


def pytest_collection_modifyitems(
    config: pytest.Config, items: list[pytest.Item]
) -> None:
    """Enforce the three destructive gates at collection time.

    Gate 1 is the ``-m "not destructive"`` selector (user-provided).
    Gate 2 is HOOK_CONTEXT → force-skip even if the marker was selected.
    Gate 3 is evaluated inside each test via ``require_destructive`` fixture.
    """
    hook_context = os.environ.get(HOOK_CONTEXT_ENV, "").strip()

    if not hook_context:
        # No hook context → developer-driven run, let the -m selector decide.
        return

    # Hook context active (sprint_end or cron) → destructive tests are hard-skipped
    # regardless of whether the user passed -m destructive. Gate 2 fires here.
    skip_marker = pytest.mark.skip(
        reason=(
            f"destructive tests hard-skipped under E2E_HOOK_CONTEXT={hook_context!r}. "
            "Run manually with no hook context to execute."
        )
    )
    for item in items:
        if "destructive" in {m.name for m in item.iter_markers()}:
            item.add_marker(skip_marker)


# ───────────────────────── session-scoped fixtures ─────────────────────────

@pytest.fixture(scope="session")
def run_scope() -> RunScope:
    run_id = f"{socket.gethostname()}-{os.getpid()}-{uuid.uuid4().hex[:8]}"
    sprint_id = os.environ.get("SPRINT_ID") or _detect_sprint_id()
    return RunScope(run_id=run_id, sprint_id=sprint_id)


def _detect_sprint_id() -> str | None:
    """Read the newest sprints/sprint-NNN.md filename as the current sprint."""
    repo_root = Path(__file__).parent.parent.parent
    sprint_dir = repo_root / "sprints"
    if not sprint_dir.is_dir():
        return None
    candidates = sorted(sprint_dir.glob("sprint-*.md"), reverse=True)
    for c in candidates:
        stem = c.stem.removeprefix("sprint-")
        if stem.isdigit():
            return stem
    return None


@pytest.fixture(scope="session")
def urls() -> dict[str, str]:
    return service_urls()


@pytest.fixture(scope="session")
def service_health(urls: dict[str, str]) -> dict[str, ServiceHealth]:
    return {
        "odin": probe("odin", urls["odin"]),
        "mimir": probe("mimir", urls["mimir"]),
        "muninn": probe("muninn", urls["muninn"]),
        "voice": probe("voice", urls["voice"]),
        "mcp_http": probe("mcp_http", urls["mcp_http"]),
    }


@pytest.fixture(scope="session")
def vault_token() -> str:
    token = os.environ.get("MIMIR_VAULT_CLIENT_TOKEN", "").strip()
    if not token:
        pytest.skip("MIMIR_VAULT_CLIENT_TOKEN not set — vault tests require it")
    return token


@pytest.fixture(scope="session")
def odin_client(urls: dict[str, str]) -> OdinClient:
    return OdinClient(urls["odin"])


@pytest.fixture(scope="session")
def mimir_client(urls: dict[str, str]) -> MimirClient:
    token = os.environ.get("MIMIR_VAULT_CLIENT_TOKEN", "")
    return MimirClient(urls["mimir"], vault_token=token or None)


@pytest.fixture(scope="session")
def muninn_client(urls: dict[str, str]) -> MuninnClient:
    return MuninnClient(urls["muninn"])


@pytest.fixture(scope="session")
def mcp_client(urls: dict[str, str]) -> McpHttpClient:
    return McpHttpClient(urls["mcp_http"])


# ───────────────────────── function-scoped fixtures ────────────────────────

@pytest.fixture
def clean_test_engrams(
    request: pytest.FixtureRequest,
    run_scope: RunScope,
    mimir_client: MimirClient,
) -> CleanScope:
    """Provide a unique tag for this test; purge any engrams carrying it on teardown."""
    scope = CleanScope(
        tag=run_scope.tag(request.node.name),
        _mimir=mimir_client,
    )
    yield scope
    # Teardown — best-effort; failures here must not mask real test assertions.
    try:
        scope.purge()
    except Exception:
        pass


@pytest.fixture
def require_destructive() -> None:
    """Gate 3 — each destructive test calls this fixture to self-skip if not opted in.

    Even if a developer runs ``pytest -m destructive`` without E2E_DESTRUCTIVE=1,
    this fixture skips at test-enter. Belt and suspenders and a safety net.
    """
    if os.environ.get(DESTRUCTIVE_ENV) != "1":
        pytest.skip(
            f"destructive test requires {DESTRUCTIVE_ENV}=1 env var "
            "(not set — skipping to avoid real side effects)."
        )
    if os.environ.get(HOOK_CONTEXT_ENV):
        pytest.skip(
            f"destructive test refuses to run under E2E_HOOK_CONTEXT "
            f"({os.environ[HOOK_CONTEXT_ENV]!r})."
        )


# ───────────────────────── required_services marker support ────────────────

def pytest_runtest_setup(item: pytest.Item) -> None:
    """Skip tests whose required_services marker lists an unreachable service."""
    marker = item.get_closest_marker("required_services")
    if marker is None:
        return

    required = set(marker.args)
    health_map: dict[str, ServiceHealth] = {
        name: probe(name, url) for name, url in service_urls().items() if name in required
    }
    down = [h for h in health_map.values() if not h.ok]
    if down:
        names = ", ".join(f"{h.name} ({h.detail})" for h in down)
        pytest.skip(f"required service(s) unreachable: {names}")


# ───────────────────────── run-id visibility in logs ───────────────────────

def pytest_report_header(config: pytest.Config) -> list[str]:
    return [
        f"yggdrasil e2e: hostname={socket.gethostname()} pid={os.getpid()}",
        f"hook_context={os.environ.get(HOOK_CONTEXT_ENV, '(none)')} "
        f"destructive={os.environ.get(DESTRUCTIVE_ENV, '0')} "
        f"parallel_ok={os.environ.get(PARALLEL_OK_ENV, '0')}",
        f"lockfile={LOCKFILE}",
    ]
