---
description: Pull the nightly production snapshot into data/prod_snapshots/YYYYMMDD/ (idempotent, read-only on prod).
---

Fetch today's production snapshot. Production is the source of truth; here you only
COPY it down — never write back to prod.

1. Compute `DATE=YYYYMMDD` (`date +%Y%m%d`). Target dir: `data/prod_snapshots/<DATE>/`.
2. If the target dir already exists and is non-empty, do nothing and report "already
   pulled" (idempotent — safe to re-run).
3. Otherwise create it and run the project's export to land the snapshot there:
   `{{SNAPSHOT_EXPORT}}`
   (this should dump `{{SOURCE_TABLES}}` as SQLite/JSONL/parquet into the dir).
4. Sanity-check: list the files and row counts you fetched. If empty or the export
   failed, stop and report the error and the exact command output — do not fabricate
   a snapshot.

Report: the snapshot path, file list, and per-table row counts. Optionally pass a
date as `$ARGUMENTS` to backfill a specific day instead of today.
