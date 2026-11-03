//! Pure segmenting logic: how a file of known size is cut into byte ranges,
//! how progress is tracked per range, and how a slow range is split ("work stealing").
//!
//! All ranges are inclusive (`start..=end`) and `done` counts the contiguous bytes already
//! written from `start`, so a range can always be resumed at `start + done`.

use serde::{Deserialize, Serialize};

/// Upper bound on segments per job (also bounds the on-disk state).
pub const MAX_SEGMENTS: usize = 64;

/// One byte range of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    /// Last byte of the range (inclusive).
    pub end: u64,
    /// Bytes already written, counted from `start`.
    pub done: u64,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Segment {
            start,
            end,
            done: 0,
        }
    }
    /// Number of bytes in the range.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
    /// Always false: a range holds at least one byte.
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.done)
    }
    /// Next byte to fetch.
    pub fn offset(&self) -> u64 {
        self.start + self.done
    }
    pub fn is_complete(&self) -> bool {
        self.done >= self.len()
    }
}

/// Splits `[0, total)` into at most `nseg` balanced ranges of at least `min_seg` bytes each
/// (sizes differ by at most one byte). Returns no segments for an empty file.
pub fn plan(total: u64, nseg: usize, min_seg: u64) -> Vec<Segment> {
    if total == 0 {
        return Vec::new();
    }
    let by_size = (total / min_seg.max(1)).max(1);
    let n = (nseg.clamp(1, MAX_SEGMENTS) as u64).min(by_size);
    let (base, rem) = (total / n, total % n);
    let mut pos = 0;
    (0..n)
        .map(|i| {
            let len = base + u64::from(i < rem);
            let s = Segment::new(pos, pos + len - 1);
            pos += len;
            s
        })
        .collect()
}

/// Bytes written over all segments.
pub fn done_total(segs: &[Segment]) -> u64 {
    segs.iter().map(|s| s.done).sum()
}

/// True if the segments tile `[0, total)` exactly (in any order, no gaps or overlaps) and
/// every `done` lies within its range. Used to decide whether saved progress can be trusted.
pub fn validate(segs: &[Segment], total: u64) -> bool {
    if total == 0 {
        return segs.is_empty();
    }
    if segs.is_empty() || segs.len() > MAX_SEGMENTS {
        return false;
    }
    let mut sorted = segs.to_vec();
    sorted.sort_by_key(|s| s.start);
    let mut pos = 0u64;
    for s in &sorted {
        if s.start != pos || s.end < s.start || s.done > s.len() {
            return false;
        }
        pos = s.end + 1;
    }
    pos == total
}

/// Work stealing: splits the candidate segment with the most bytes left, provided it has at least
/// `2 * min_piece` left. The victim keeps the first half of what remains; the new segment (pushed
/// to the end of `segs`) takes the rest. `candidate(i)` restricts which segments may be split
/// (the engine only offers segments that are currently being downloaded).
/// Returns `(victim, created)` indices.
pub fn steal(
    segs: &mut Vec<Segment>,
    min_piece: u64,
    candidate: impl Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    if segs.len() >= MAX_SEGMENTS || min_piece == 0 {
        return None;
    }
    let (victim, rem) = segs
        .iter()
        .enumerate()
        .filter(|(i, _)| candidate(*i))
        .map(|(i, s)| (i, s.remaining()))
        .max_by_key(|&(_, r)| r)?;
    if rem < 2 * min_piece {
        return None;
    }
    let off = segs[victim].offset();
    let keep = rem / 2;
    let tail = Segment::new(off + keep, segs[victim].end);
    segs[victim].end = off + keep - 1;
    segs.push(tail);
    Some((victim, segs.len() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_tiles_exactly_and_is_balanced() {
        for total in [1u64, 2, 7, 100, 1_000_003, 1 << 33] {
            for n in [1usize, 2, 3, 8, 64] {
                let s = plan(total, n, 1);
                assert!(!s.is_empty() && s.len() <= n);
                assert!(validate(&s, total), "total {total} n {n}");
                let lens: Vec<u64> = s.iter().map(Segment::len).collect();
                assert!(lens.iter().max().unwrap() - lens.iter().min().unwrap() <= 1);
            }
        }
    }

    #[test]
    fn plan_respects_minimum_size_and_edge_cases() {
        assert_eq!(plan(10 << 20, 8, 1 << 20).len(), 8);
        assert_eq!(plan(3 << 20, 8, 1 << 20).len(), 3);
        assert_eq!(plan(500, 8, 1 << 20).len(), 1);
        assert!(plan(0, 8, 1).is_empty());
        assert_eq!(plan(10_000, 1000, 1).len(), MAX_SEGMENTS);
    }

    #[test]
    fn plan_exact_ranges() {
        let s = plan(10, 3, 1);
        assert_eq!(
            s.iter().map(|x| (x.start, x.end)).collect::<Vec<_>>(),
            vec![(0, 3), (4, 6), (7, 9)]
        );
    }

    #[test]
    fn progress_arithmetic() {
        let mut s = Segment::new(100, 199);
        assert_eq!((s.len(), s.remaining(), s.offset()), (100, 100, 100));
        s.done = 40;
        assert_eq!((s.remaining(), s.offset()), (60, 140));
        assert!(!s.is_complete());
        s.done = 100;
        assert!(s.is_complete());
        assert_eq!(
            done_total(&[
                s,
                Segment {
                    done: 5,
                    ..Segment::new(0, 9)
                }
            ]),
            105
        );
    }

    #[test]
    fn validate_rejects_bad_layouts() {
        let good = vec![
            Segment::new(0, 49),
            Segment::new(50, 99),
            Segment::new(100, 149),
        ];
        assert!(validate(&good, 150));
        assert!(!validate(&good, 151), "too short");
        let mut gap = good.clone();
        gap[1].start = 51;
        assert!(!validate(&gap, 150));
        let mut overlap = good.clone();
        overlap[1].start = 49;
        assert!(!validate(&overlap, 150));
        let mut over_done = good.clone();
        over_done[1].done = 51;
        assert!(!validate(&over_done, 150));
        let shuffled = vec![good[2], good[0], good[1]];
        assert!(validate(&shuffled, 150), "order does not matter");
        assert!(validate(&[], 0));
        assert!(!validate(&[], 5));
    }

    #[test]
    fn steal_splits_the_largest_remaining_range() {
        let mut s = vec![
            Segment {
                done: 100,
                ..Segment::new(0, 99)
            },
            Segment {
                done: 100,
                ..Segment::new(100, 999)
            },
        ];
        let (v, c) = steal(&mut s, 100, |i| i == 1).unwrap();
        assert_eq!((v, c), (1, 2));
        assert_eq!(s[1].end, 599);
        assert_eq!((s[2].start, s[2].end, s[2].done), (600, 999, 0));
        assert!(validate(&s, 1000));
        assert_eq!(s[1].remaining(), 400);
    }

    #[test]
    fn steal_refuses_small_or_excluded_work() {
        let mut s = vec![Segment::new(0, 99)];
        assert!(steal(&mut s, 60, |_| true).is_none(), "100 < 2*60");
        assert!(steal(&mut s, 50, |_| false).is_none(), "not a candidate");
        assert!(steal(&mut s, 50, |_| true).is_some());
    }

    #[test]
    fn repeated_stealing_keeps_the_tiling() {
        let mut s = plan(1 << 20, 1, 1);
        let mut n = 0;
        while steal(&mut s, 1000, |_| true).is_some() {
            n += 1;
        }
        assert!(n > 5);
        assert!(validate(&s, 1 << 20));
    }
}
