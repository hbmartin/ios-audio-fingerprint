//! Hash-sequence comparison and checkpoint matching.
//!
//! Scores measure the fraction of agreeing bits between two positionally
//! aligned hash sequences: `matching_bits / (compared_hashes * 32)` over the
//! overlapping prefix. Because two *unrelated* bit streams whose bits are set
//! with probability `p` agree with probability `p^2 + (1 - p)^2 >= 0.5`, the
//! noise floor is at least 0.5 — a score near 0.5 means "chance level", not
//! "half confident". See `docs/implementation.md` (Matching) for the full
//! derivation and threshold guidance.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub timestamp: f32,
    pub score: f32,
}

/// Everything the drift search learns about the best alignment of two hash
/// sequences. [`compare_hashes_with_drift`] returns only [`score`]; the other
/// fields expose the evidence behind it (how many hashes overlapped, how many
/// bits agreed, and at which shift) without changing the public scoring path.
///
/// [`score`]: DriftComparison::score
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftComparison {
    /// Bit-agreement fraction in `[0.0, 1.0]` at the best offset; `0.0` when
    /// either input is empty.
    pub score: f32,
    /// Signed best shift in hash positions (one position spans
    /// `HASH_STRIDE_FRAMES * HOP_SIZE` samples, about 186 ms of audio).
    /// Positive means the best alignment dropped that many leading hashes of
    /// `first` (i.e. `first` leads `second`); negative means leading hashes of
    /// `second` were dropped. Offsets are searched in the order `0, +1, -1,
    /// +2, -2, …` and the first offset attaining the maximum score wins ties.
    pub best_offset: i32,
    /// Number of hash pairs compared at the best offset (the min overlap
    /// after the shift). A score computed over few hashes carries far less
    /// evidence than the same score over hundreds.
    pub compared_hashes: usize,
    /// Number of agreeing bits at the best offset, out of
    /// `compared_hashes * 32`.
    pub matching_bits: usize,
}

#[derive(Debug, Clone)]
struct Checkpoint {
    timestamp: f32,
    hashes: Vec<u32>,
    /// Carried through the public `add` API for callers' bookkeeping; scoring
    /// is currently length-based and does not weight by duration.
    duration: f32,
}

#[derive(Debug, Clone, Default)]
pub struct CheckpointMatcher {
    checkpoints: Vec<Checkpoint>,
    max_drift: u32,
}

pub fn compare_hashes(first: &[u32], second: &[u32]) -> f32 {
    compare_at_offset(first, second, 0, 0)
}

/// The maximum score over shifts of up to `max_drift` hash positions in both
/// directions (`2 * max_drift + 1` comparisons), searched in the order `0,
/// +1, -1, +2, -2, …`. Taking a max over more comparisons mildly inflates the
/// scores of *non*-matching sequences, so thresholds and `max_drift` should
/// be tuned together.
pub fn compare_hashes_with_drift(first: &[u32], second: &[u32], max_drift: u32) -> f32 {
    compare_hashes_with_drift_detailed(first, second, max_drift).score
}

/// The drift search behind [`compare_hashes_with_drift`], reporting the best
/// offset and the raw evidence counts alongside the score it returns.
pub fn compare_hashes_with_drift_detailed(
    first: &[u32],
    second: &[u32],
    max_drift: u32,
) -> DriftComparison {
    if first.is_empty() || second.is_empty() {
        return DriftComparison {
            score: 0.0,
            best_offset: 0,
            compared_hashes: 0,
            matching_bits: 0,
        };
    }

    let mut best = comparison_at_offset(first, second, 0, 0, 0);
    let drift_limit = (max_drift as usize).min(first.len()).min(second.len());
    for drift in 1..=drift_limit {
        for (first_start, second_start, offset) in
            [(drift, 0, drift as i32), (0, drift, -(drift as i32))]
        {
            let candidate = comparison_at_offset(first, second, first_start, second_start, offset);
            if candidate.score > best.score {
                best = candidate;
            }
        }
    }
    best.score = best.score.clamp(0.0, 1.0);
    best
}

fn comparison_at_offset(
    first: &[u32],
    second: &[u32],
    first_start: usize,
    second_start: usize,
    offset: i32,
) -> DriftComparison {
    let (matching_bits, count) =
        count_matching_bits_at_offset(first, second, first_start, second_start);
    let score = if count == 0 {
        0.0
    } else {
        matching_bits as f32 / (count * 32) as f32
    };
    DriftComparison {
        score,
        best_offset: offset,
        compared_hashes: count,
        matching_bits,
    }
}

fn compare_at_offset(
    first: &[u32],
    second: &[u32],
    first_start: usize,
    second_start: usize,
) -> f32 {
    comparison_at_offset(first, second, first_start, second_start, 0).score
}

/// Agreeing bits and compared-hash count over the overlap of `first` and
/// `second` after dropping the given leading counts. Returns `(0, 0)` when
/// either start is out of bounds or the overlap is empty.
fn count_matching_bits_at_offset(
    first: &[u32],
    second: &[u32],
    first_start: usize,
    second_start: usize,
) -> (usize, usize) {
    if first_start >= first.len() || second_start >= second.len() {
        return (0, 0);
    }

    let count = (first.len() - first_start).min(second.len() - second_start);
    if count == 0 {
        return (0, 0);
    }

    let first = &first[first_start..first_start + count];
    let second = &second[second_start..second_start + count];

    // Compare two hashes per popcount by packing adjacent u32 pairs into u64s.
    let mut first_pairs = first.chunks_exact(2);
    let mut second_pairs = second.chunks_exact(2);
    let mut matching_bits = 0usize;
    for (pair_a, pair_b) in (&mut first_pairs).zip(&mut second_pairs) {
        let a = pair_a[0] as u64 | (pair_a[1] as u64) << 32;
        let b = pair_b[0] as u64 | (pair_b[1] as u64) << 32;
        matching_bits += (!(a ^ b)).count_ones() as usize;
    }
    for (a, b) in first_pairs.remainder().iter().zip(second_pairs.remainder()) {
        matching_bits += (!(a ^ b)).count_ones() as usize;
    }

    (matching_bits, count)
}

impl CheckpointMatcher {
    pub fn new() -> Self {
        Self::with_drift(0)
    }

    pub fn with_drift(max_drift: u32) -> Self {
        Self {
            checkpoints: Vec::new(),
            max_drift,
        }
    }

    pub fn add(&mut self, timestamp: f32, hashes: Vec<u32>, duration: f32) {
        self.checkpoints.push(Checkpoint {
            timestamp,
            hashes,
            duration,
        });
    }

    pub fn clear(&mut self) {
        self.checkpoints.clear();
    }

    pub fn count(&self) -> u32 {
        self.checkpoints.len().min(u32::MAX as usize) as u32
    }

    pub fn set_drift(&mut self, max_drift: u32) {
        self.max_drift = max_drift;
    }

    pub fn find_top_matches(&self, query_hashes: &[u32], max_results: u32) -> Vec<MatchResult> {
        if max_results == 0 {
            return Vec::new();
        }

        let mut scored: Vec<(usize, MatchResult)> = self
            .checkpoints
            .iter()
            .enumerate()
            .map(|(index, checkpoint)| {
                let _stored_duration = checkpoint.duration;
                (
                    index,
                    MatchResult {
                        timestamp: checkpoint.timestamp,
                        score: compare_hashes_with_drift(
                            query_hashes,
                            &checkpoint.hashes,
                            self.max_drift,
                        ),
                    },
                )
            })
            .collect();

        let ordering = |(left_index, left): &(usize, MatchResult),
                        (right_index, right): &(usize, MatchResult)|
         -> Ordering {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.timestamp.total_cmp(&right.timestamp))
                .then_with(|| left_index.cmp(right_index))
        };

        // Only the leading `max_results` entries are returned, so push the
        // winners to the front with an O(n) selection and sort just that
        // prefix. The comparator is a total order (the insertion index breaks
        // every tie), so the result is identical to fully sorting.
        let keep = (max_results as usize).min(scored.len());
        if keep < scored.len() {
            scored.select_nth_unstable_by(keep, ordering);
            scored.truncate(keep);
        }
        scored.sort_by(ordering);

        scored
            .into_iter()
            .take(max_results as usize)
            .map(|(_, result)| result)
            .collect()
    }
}
