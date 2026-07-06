# Interpreting Match Scores

Understand what comparison scores measure, why unrelated audio scores near
0.5, and how to choose thresholds and drift settings on principle instead of
by feel.

## What the score measures

``compareHashes(hashes1:hashes2:)``, ``compareHashesWithDrift(hashes1:hashes2:maxDrift:)``,
and ``MatchResult/score`` all report the same quantity: the fraction of
agreeing bits between two positionally aligned hash sequences,

```
score = matchingBits / (comparedHashes × 32)
```

where `comparedHashes` is the length of the overlap (the shorter of the two
sequences after any shift). Scores are always in `[0, 1]`; comparing an empty
sequence scores `0`.

## The noise floor is 0.5, not 0

Two *unrelated* hash streams do not score near zero. If each bit is set with
probability `p`, two independent bits agree with probability
`p² + (1 − p)²`, which is minimized at **0.5** when `p = 0.5` and only rises
as bits become biased — and fingerprint bits are biased. Unrelated audio
therefore typically scores in the **0.5–0.6** range.

Read scores accordingly:

- **≈ 0.5** — chance level. No evidence of a match, not "half confident".
- **< ~0.55** — indistinguishable from noise for typical sequence lengths.
- **~0.65 and above** — a confident match for typical spoken/music content.
- **≫ 0.8** — the same content re-encoded, level-shifted, or lightly
  processed.

Calibrate the exact cutoffs on your own corpus: content style, sequence
length, and drift settings all move the distributions. Short comparisons are
noisy — an `n`-hash comparison has a standard deviation of roughly
`0.5 / √(32 n)` at the noise floor (about ±0.028 at 10 hashes, ±0.003 at
1,000), so demand longer overlaps or higher scores before trusting a match on
a short window.

## Drift: units and effect on scores

One drift unit is **one hash position ≈ 186 ms of audio**: hashes are emitted
every `HASH_STRIDE_FRAMES × HOP_SIZE = 2 × 1,024` samples at the fixed
11,025 Hz analysis rate (2,048 / 11,025 ≈ 185.8 ms).

``compareHashesWithDrift(hashes1:hashes2:maxDrift:)`` returns the **maximum**
score over `2 × maxDrift + 1` shifted comparisons (offsets `0, +1, −1, +2,
−2, …`). Two consequences:

- **Misalignment tolerance.** A query that starts off the stored grid by up
  to `maxDrift × 0.186 s` can still align. Rule of thumb:
  `maxDrift ≈ ⌈worstCaseMisalignmentSeconds / 0.186⌉`. For example,
  checkpoints on a 2 s grid queried from arbitrary positions are at most 1 s
  off-grid, so `maxDrift = 6` absorbs any query offset.
- **Mild score inflation for non-matches.** Taking a max over more
  comparisons raises the expected best *noise* score slightly, so raising
  `maxDrift` should be paired with re-checking thresholds.

### What drift does not do

The drift search is a single global shift per comparison. It does **not**
track timing trends across successive queries, enforce monotonic progress
between ``CheckpointMatcher/findTopMatches(queryHashes:maxResults:)`` calls,
or align at resolutions finer than one hash position (~186 ms). Applications
that need cross-window consistency filtering (for example, following playback
through dynamically inserted content) must implement it on top of the
per-window scores.

## Matching against checkpoints

``CheckpointMatcher/findTopMatches(queryHashes:maxResults:)`` scores **every**
stored checkpoint with the drift search at the current
``CheckpointMatcher/setDrift(maxDrift:)`` value on each call — cost grows
with `checkpoints × queryLength × maxDrift` — then returns the top
`maxResults` by score. `maxResults` only truncates the returned list; it does
not reduce the scoring work.
