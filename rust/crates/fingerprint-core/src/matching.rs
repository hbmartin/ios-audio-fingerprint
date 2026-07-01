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

    let mut matching_bits = 0usize;
    for index in 0..count {
        matching_bits +=
            (!(first[first_start + index] ^ second[second_start + index])).count_ones() as usize;
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

        scored.sort_by(|(left_index, left), (right_index, right)| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.timestamp.total_cmp(&right.timestamp))
                .then_with(|| left_index.cmp(right_index))
                .then(Ordering::Equal)
        });

        scored
            .into_iter()
            .take(max_results as usize)
            .map(|(_, result)| result)
            .collect()
    }
}
