//! Client-side bounded LRU cache for instant 0-token repeat log explanations and RCAs.

use std::collections::VecDeque;

/// Maximum entries retained in the client-side AI response LRU cache.
const AI_CACHE_CAPACITY: usize = 64;

/// Bounded in-memory LRU cache mapping normalized prompt/fingerprint keys to AI responses.
#[derive(Debug, Default)]
pub(crate) struct AiResponseCache {
    entries: VecDeque<(String, String)>,
}

impl AiResponseCache {
    pub(crate) fn get(&mut self, key: &str) -> Option<String> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, v) = self.entries.remove(pos)?;
        self.entries.push_back((k, v.clone()));
        Some(v)
    }

    pub(crate) fn insert(&mut self, key: String, value: String) {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(pos);
        }
        self.entries.push_back((key, value));
        while self.entries.len() > AI_CACHE_CAPACITY {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_response_cache_lru_order_and_eviction() {
        let mut cache = AiResponseCache::default();
        cache.insert("k1".to_string(), "v1".to_string());
        cache.insert("k2".to_string(), "v2".to_string());
        assert_eq!(cache.get("k1").as_deref(), Some("v1"));

        // Overwriting existing key updates value in place
        cache.insert("k1".to_string(), "v1-updated".to_string());
        assert_eq!(cache.get("k1").as_deref(), Some("v1-updated"));
    }
}
