use std::collections::HashMap;

use skotos_domain::{EvalFlag, EvalSegment};

/// Pure synchronous, zero-I/O read interface used by the evaluation engine.
pub trait EvalStore {
    fn get_flag(&self, key: &str) -> Option<&EvalFlag>;
    fn get_segment(&self, key: &str) -> Option<&EvalSegment>;
}

/// Simple in-memory [`EvalStore`] backed by hash maps (used in simulation,
/// Delivery Edge caches, golden tests, and unit tests).
#[derive(Debug, Default, Clone)]
pub struct InMemoryEvalStore {
    pub flags: HashMap<String, EvalFlag>,
    pub segments: HashMap<String, EvalSegment>,
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
}

impl EvalStore for InMemoryEvalStore {
    fn get_flag(&self, key: &str) -> Option<&EvalFlag> {
        self.flags.get(key)
    }

    fn get_segment(&self, key: &str) -> Option<&EvalSegment> {
        self.segments.get(key)
    }
}
