# Published reports

This branch is written by CI only; don't push to it by hand. It holds the
latest reports of `master`, which the Observatory reads:

- `generation.coverage.json`: line and function coverage per crate, from
  `.github/workflows/reports.yml` on every push to `master`. Format: the
  Observatory's `DATA.md` (`coverage`).

Each report names the `master` commit it measured. Every publish is one
commit here, so `git log -- generation.coverage.json` is the series.
