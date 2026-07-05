use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub timestamp: f32,
    pub score: f32,
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

pub fn compare_hashes_with_drift(first: &[u32], second: &[u32], max_drift: u32) -> f32 {
    if first.is_empty() || second.is_empty() {
        return 0.0;
    }

    let mut best = compare_hashes(first, second);
    let drift_limit = (max_drift as usize).min(first.len()).min(second.len());
    for drift in 1..=drift_limit {
        best = best.max(compare_at_offset(first, second, drift, 0));
        best = best.max(compare_at_offset(first, second, 0, drift));
    }
    best.clamp(0.0, 1.0)
}

fn compare_at_offset(
    first: &[u32],
    second: &[u32],
    first_start: usize,
    second_start: usize,
) -> f32 {
    if first_start >= first.len() || second_start >= second.len() {
        return 0.0;
    }

    let count = (first.len() - first_start).min(second.len() - second_start);
    if count == 0 {
        return 0.0;
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

    matching_bits as f32 / (count * 32) as f32
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
