# Performance on a small host (Render free tier: 0.1 CPU, 512 MB)

Everything below was measured with the real code and the real PaddleOCR models. The "0.1 CPU / 512 MB" runs used a Linux cgroup with the same limits as Render's free tier (`CPUQuota=10%`, `MemoryMax=512M`, swap off; `systemd-run --user --scope`), so the kernel throttles and kills exactly as it would there. No run was killed by the memory limit.

## What an applicant or a staff member waits for (0.1 CPU / 512 MB)

| Step | Time on 0.1 CPU | Same step on a normal machine |
| --- | --- | --- |
| OCR models load (once, in the background at start) | 3.5 s | 0.3 s |
| **Instant check** of one screenshot (profile card / standings / result screen) | **5-7 s** | 0.35-0.55 s |
| Licence + team-event screenshot in one upload -> **ticket created** (whole flow incl. DM mirror, check, thread) | **11.8 s** | ~1 s |
| Licence only -> "do you have event screenshots?" question | **5.4 s** | ~0.5 s |
| **Ticket closed**: log embed, transcript, lock/archive | **0.5 s** | 0.1 s |
| ...then the one background reading that completes the log (licence + applicant's event row) | ~60 s in the background | ~2 s |
| Spreadsheet: read one cropped standings page (28 rows, one text pass) | 50 s | 3.9 s |
| Spreadsheet: read the full team-event screen / the result screen | 21 s / 16 s | 1.7 s / 1.2 s |
| Five screenshots -> 96-player XLSX + spreadsheet image + chart | **3 min (180-210 s)** | 14 s |
| Rebuild outputs from stored readings (after a correction) | 2.3 s | 0.2 s |

Applicants are never held for long: the instant check is one text pass on its own lane with a deadline (`DCA_OCR_QUICK_SECS`, default 20 s). A check that runs out of time lets the upload through on trust and tells the recruiters. When an accepted ticket is closed the log is posted at once and completed by one background reading. The spreadsheet is built in the background after the grouping window; each screenshot is read in one text pass.

## Memory

| | MB |
| --- | --- |
| Process after start | 11 |
| Models loaded (detector + Latin + multilingual) | 90 |
| After a first screenshot has been read | ~220 |
| Steady state while working (recycled below ~205 MB before each job) | 250-450 |
| **Peak measured by the cgroup**, 8 whole flows incl. the 5-screenshot spreadsheet | **446 MB** |
| Peak, benchmark harness (everything in one process, plus the test rig) | 482 MB |

The limit is 512 MB, so there is headroom but not a lot. Three things keep the process there: ONNX Runtime keeps every buffer it ever needed, so the sessions are rebuilt (and the heap trimmed) whenever the process has grown past 40% of the container's limit (`DCA_OCR_RECYCLE_MB`); the eight language models other than Latin / multilingual are loaded on demand and at most two stay in memory (`DCA_OCR_KEEP_SCRIPTS`); and the allocator is limited to one arena (`MALLOC_ARENA_MAX=1`, also set by the bot at start). A host with more memory simply recycles less.

## CPU

- The container is told to use one inference thread (`DCA_OCR_THREADS`, default: what the container may use). With a 0.1 CPU quota that is the only sensible setting: on the benchmark machine the default 12 threads spend 3x the CPU for 2x the speed, which a quota turns into pure throttling.
- A 0.1 CPU quota means a tenth of the wall-clock speed of one core, so every number above is simply the one-core cost times ten (one core: instant check 0.35-0.55 s CPU; spreadsheet of five screenshots 14.8 s CPU).
- Reading cost is dominated by text recognition of name crops: the multilingual model only runs on lines the Latin model is unsure of, and the instant check stops reading as soon as it can tell what the screenshot is.

## Same benchmark, three environments

| Stage | 12 threads, no limit | 1 thread, no limit | 0.1 CPU / 512 MB |
| --- | --- | --- | --- |
| instant check, licence | 0.35 s | 0.35 s | 5.1 s |
| instant check, standings page | 0.5 s | 0.5 s | 7.2 s |
| read one standings page (one pass) | 3.9 s | 3.9 s | 50 s |
| 5-screenshot spreadsheet | 9.7 s (multi-threaded) | 14.3 s | 180 s |
| peak memory (VmHWM / cgroup) | 505 MB | 497 MB | 512 MB / 482 MB |

## Accuracy under the limits

The 96-player sample event (five screenshots, `bot/fixtures/samples`) reads the same on a full machine and on the 0.1 CPU / 512 MB container: all 96 ranks and scores exact, event name, both team scores and the opponent team from the result screen, 91 of 96 names (the misses are circled digits/letters, a superscript, and a Cyrillic name with a flag emoji).

## Reproduce

```bash
cd bot/native
cargo test -p dca-bot --release --no-run              # prints the test binary path
BIN=target/release/deps/dca_bot-<hash>
$BIN bench --ignored --nocapture --test-threads=1      # the stage table above
systemd-run --user --scope -p CPUQuota=10% -p MemoryMax=512M -p MemorySwapMax=0 \
  env MALLOC_ARENA_MAX=1 $BIN bench --ignored --nocapture --test-threads=1
```

`flow_tests::` (apply, auto ticket, close, spreadsheet) can be run the same way to time the whole flows.
