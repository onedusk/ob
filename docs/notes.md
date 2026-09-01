  - A --quick or -q flag for inline patterns without YAML would make ad-hoc searches frictionless: ob scan . -q "font-bold|font-medium"
  - Could be the best of both worlds

## Parallel directory walking (audit P5) — measured, rejected (2026-09-01)

Tried replacing the serial `WalkBuilder::build()` file collection in scan's
`collect_files` with `WalkBuilder::build_parallel()` (mutex-collected Vec,
warn-and-continue on per-entry errors, extension filter, --hidden/--no-ignore,
and --workers honored — exact serial semantics, order unspecified). The full
test suite passed with the change. Reverted per the benchmark gate below.

Method: release builds, macOS (Darwin 25), warm cache, `scan -e 'TODO'
<corpus> -o <file>`, 2 warmup runs then timed runs (hyperfine not installed;
Python perf_counter harness), including interleaved A/B runs. Machine load
average 3-5 during measurement (noisy).

Corpora:
- fixture: 40,000 small .rs files across 2,000 dirs (3-level nesting), 400
  files containing TODO
- workspace: real multi-package tree, ~930 files after gitignore filtering

End-to-end wall time (serial -> parallel, median):
- fixture, sequential experiments (n=7 each): 1852.3 ms -> 1678.8 ms (-9.4%)
- fixture, interleaved A/B (n=15 each): 1496.7 ms -> 1369.6 ms (-8.5%)
  (min 1296.7 ms -> 1155.7 ms)
- workspace (n=7 each): 22.5 ms -> 21.0 ms (-6.7%)
- workspace, interleaved A/B (n=10 each): 24.2 ms -> 22.4 ms (-7.4%)

Collection phase isolated (`-x zzz`: walks the whole tree, scans nothing;
interleaved, n=12 each): 122.8 ms -> 61.4 ms median, 2.0x faster, tight
variance. The parallel walk itself is a clear win.

Why rejected: the adoption gate required >=10% median end-to-end improvement
on at least one corpus. Collection is too small a fraction of scan's runtime
for that: the walk saves at most ~61 ms of a ~1.4-1.9 s fixture run (~4%
steady-state; larger observed deltas were load drift) and ~1-2 ms of a ~22 ms
workspace run. With warm caches the bar is unreachable on these corpora, so
the change was reverted rather than shipped as unproven complexity.

Revisit if scan profiles change: cold caches / network filesystems, or trees
where walking dominates (very deep nesting, heavy ignore rules, extension
filters excluding most files). The 2x collection-phase speedup is real; it is
the end-to-end share that is too small today.
