//! A byte-budgeted LRU cache: inserting evicts the least recently used entries until the new one
//! fits, so the total never exceeds the budget. An entry larger than the whole budget is refused.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// A least-recently-used cache holding at most `budget` bytes.
#[derive(Debug)]
pub struct Lru<K, V> {
    budget: usize,
    used: usize,
    tick: u64,
    entries: HashMap<K, (V, usize, u64)>,
    /// Last use → key, oldest first.
    order: BTreeMap<u64, K>,
}

impl<K: Eq + Hash + Clone, V> Lru<K, V> {
    pub fn new(budget: usize) -> Lru<K, V> {
        Lru { budget, used: 0, tick: 0, entries: HashMap::new(), order: BTreeMap::new() }
    }

    /// Bytes held now; never more than the budget.
    pub fn used(&self) -> usize {
        self.used
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inserts (or replaces) `key` costing `bytes`, evicting least recently used entries to make
    /// room. Returns the evicted keys; an entry larger than the budget is not inserted (and a
    /// previous value for its key is dropped), and its own key is returned.
    pub fn insert(&mut self, key: K, value: V, bytes: usize) -> Vec<K> {
        let mut evicted = Vec::new();
        self.remove(&key);
        if bytes > self.budget {
            evicted.push(key);
            return evicted;
        }
        while self.used + bytes > self.budget {
            let Some((_, oldest)) = self.order.pop_first() else { break };
            if let Some((_, size, _)) = self.entries.remove(&oldest) {
                self.used -= size;
            }
            evicted.push(oldest);
        }
        self.tick += 1;
        self.order.insert(self.tick, key.clone());
        self.entries.insert(key, (value, bytes, self.tick));
        self.used += bytes;
        evicted
    }

    /// The value for `key`, marking it most recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        let (_, _, last) = self.entries.get_mut(key)?;
        self.order.remove(last);
        *last = tick;
        self.order.insert(tick, key.clone());
        self.entries.get(key).map(|(v, _, _)| v)
    }

    /// Whether `key` is cached, without changing its recency.
    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (value, size, last) = self.entries.remove(key)?;
        self.order.remove(&last);
        self.used -= size;
        Some(value)
    }
}
