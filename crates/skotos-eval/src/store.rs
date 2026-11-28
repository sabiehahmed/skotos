use std::collections::HashMap;

use skotos_domain::{BigSegmentSnapshot, EvalFlag, EvalSegment};

/// Pure synchronous, zero-I/O read interface used by the evaluation engine.
pub trait EvalStore {
    fn get_flag(&self, key: &str) -> Option<&EvalFlag>;
    fn get_segment(&self, key: &str) -> Option<&EvalSegment>;

    /// Optional high-scale Big Segment membership query (`Some(true)` = included,
    /// `Some(false)` = excluded, `None` = not present / fall back to rules).
    fn big_segment_membership(
        &self,
        _segment_key: &str,
        _context_kind: &str,
        _context_key: &str,
    ) -> Option<bool> {
        None
    }
}

/// Simple in-memory [`EvalStore`] backed by hash maps (used in simulation,
/// Delivery Edge caches, Relay Proxy, golden tests, and unit tests).
#[derive(Debug, Default, Clone)]
pub struct InMemoryEvalStore {
    pub flags: HashMap<String, EvalFlag>,
    pub segments: HashMap<String, EvalSegment>,
    pub big_segments: HashMap<String, BigSegmentSnapshot>,
}

impl InMemoryEvalStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_flag(&mut self, flag: EvalFlag) {
        self.flags.insert(flag.key.clone(), flag);
    }

    pub fn insert_segment(&mut self, segment: EvalSegment) {
        self.segments.insert(segment.key.clone(), segment);
    }

    pub fn insert_big_segment(&mut self, snapshot: BigSegmentSnapshot) {
        self.big_segments
            .insert(snapshot.segment_key.clone(), snapshot);
    }
}

impl EvalStore for InMemoryEvalStore {
    fn get_flag(&self, key: &str) -> Option<&EvalFlag> {
        self.flags.get(key)
    }

    fn get_segment(&self, key: &str) -> Option<&EvalSegment> {
        self.segments.get(key)
    }

    fn big_segment_membership(
        &self,
        segment_key: &str,
        context_kind: &str,
        context_key: &str,
    ) -> Option<bool> {
        self.big_segments
            .get(segment_key)
            .and_then(|snap| snap.check_membership(context_kind, context_key))
    }
}
