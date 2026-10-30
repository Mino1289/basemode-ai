# Fork plan — Mino1289/basemode-ai

Status of the hardening and portability work. Upstream: `ChristopherKahler/base`.
Harness detail: [docs/harnesses.md](harnesses.md). Release signing:
[docs/releasing.md](releasing.md).

## Done

| # | Work | Commit |
|---|---|---|
| 0 | Release coordinates repointed at the fork; guard test holds all four in place | `755b6be` |
| 1 | Dashboard: loopback bind, `Host` gate, token for remote binds, no permissive CORS | `a31aaef` |
| — | Pre-existing clippy failure on current stable, so CI can pass | `91c7614` |
| 2 | Signed releases: minisign over `SHA256SUMS`, verified before unpacking; release workflow signs or publishes a draft | `e98c6ff`, `85bfc22` |
| 2b | `install.sh` checks the checksum always and the signature when `minisign` is present; `install.ps1` checks the checksum | `e7abd65` |

## Before the first release from this fork

1. `scripts/release-sign.sh keygen` (or `keygen --ci`), commit `release-key.pub`.
2. Either `gh secret set MINISIGN_SECRET_KEY < ~/.config/base/minisign.key`
   (CI signs) or leave it unset and sign each draft by hand.
3. The release workflow fails the build job while `release-key.pub` is still the
   keygen placeholder — deliberately, because a binary that cannot verify its
   own updates is a binary that can never update.

## Next

| # | Work | Why | Blocked by |
|---|---|---|---|
| 1.5 | Harness adapters (OpenCode first, then Cursor, then Copilot) | Claude Code is not installed; today's hooks are dangling | — |
| 3 | Install footprint: `pip --user`/venv, `--project` hook scope | `base install` mutates the Python environment and the global agent config | — |
| 4 | Scrub on every graph write, `.base/.gitignore` | a pasted secret in a decision lands in `.base/graph.nq`, which is meant to be committed | — |
| 5 | `SECURITY.md`, signed tags, verify-before-install docs | disclosure path and provenance | — |

## Notes

- Phase 2 is implemented and unit/integration tested, but it cannot be exercised
  end to end against a real release until the fork publishes one. The shell
  installer's four paths (valid, tampered checksum, tampered archive, bad
  signature) were exercised against a local fake release.
- Phase 1.5 is what makes the fork usable on this machine at all.
- `docs/harnesses.md` records the researched APIs and their known upstream bugs.
  Re-verify OpenCode and Copilot before implementing; both move quickly.
- Machine state on 2026-10-29: `~/.claude/settings.json` already contains base
  hooks, but no `base` binary is on PATH, so every hook is a failed spawn. This
  is the exact failure mode a fork must not ship.
