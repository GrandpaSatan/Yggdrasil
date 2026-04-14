# Yggdrasil E2E Tests (Sprint 066)

End-to-end functional tests that exercise the live Yggdrasil fleet the same way a user does: real HTTP to real services, real Postgres + Qdrant + Ollama, real engrams created and cleaned up.

## Quick start

```bash
cd yggdrasil/tests-e2e
python -m venv .venv && source .venv/bin/activate
pip install -e .
cp .env.test.example .env.test
chmod 600 .env.test
# edit .env.test with real MIMIR_VAULT_CLIENT_TOKEN

pytest -v                              # full safe suite
pytest -v -m "not slow"                # fast subset (sprint-end default)
pytest -v tests/test_memory.py         # one file
pytest -v -k "recall_roundtrip"        # one test
```

## VS Code Test Explorer

The workspace `.vscode/settings.json` wires pytest as the discovery provider. With the Python extension installed, all tests appear in the Testing sidebar (beaker icon), runnable with the play icon and debuggable with the bug icon. Select the `tests-e2e/.venv/bin/python` interpreter and VS Code auto-discovers everything.

## Execution gates

Three independent gates protect against accidental side-effects. A destructive test (VM launch, SSH deploy, real HA actuation) fires **only** when all three open:

1. Test is marked `@pytest.mark.destructive` (and therefore excluded by the default `-m "not destructive"` selector used by the sprint-end hook and cron wrapper).
2. `E2E_HOOK_CONTEXT` env var is **absent** (the sprint-end hook and cron wrapper set this to `sprint_end` or `cron` — conftest hard-skips destructive tests when present).
3. `E2E_DESTRUCTIVE=1` env var is **set** (last-ditch in-test guard via `pytest.skip(...)`).

The only path that fires destructive tests is a developer explicitly typing:

```bash
E2E_DESTRUCTIVE=1 pytest -m destructive
```

## Concurrency

Default is **serial execution**. Live Mimir's store_gate LFM2.5 queue is serial (~200ms per gate) and the Postgres pool caps at 16, so hammering the fleet with parallel workers causes 429s and DB contention that produce false-negative test failures.

- `pytest` — serial, default.
- `pytest -n 4` without `E2E_PARALLEL_OK=1` — conftest refuses to start.
- Two parallel `pytest` invocations against the same fleet — second one skips at session start via the `/tmp/yggdrasil-e2e.lock` file.

Parallel is only meaningful for the `test_concurrency.py` stress harness (marked `slow`, not part of sprint-end).

## What tests are expected to skip vs fail

- **Skip** (not a failure): required service down, `.env.test` missing a value, destructive test without gates open.
- **xfail** (not a failure): audit vulnerability not yet remediated. These are tagged with the VULN ID and marked `strict=True` so passing accidentally becomes a failure.
- **Fail**: a user-facing flow broke.

## Troubleshooting

- `FileNotFoundError: .env.test` — copy from `.env.test.example`.
- `PermissionError: .env.test is world-readable` — `chmod 600 .env.test`.
- `Another pytest run holds /tmp/yggdrasil-e2e.lock` — wait for the other run or delete the stale lockfile if no process owns it.
- Tests flaky on first run — cold Ollama loads can take ~10s; re-run once warm.
