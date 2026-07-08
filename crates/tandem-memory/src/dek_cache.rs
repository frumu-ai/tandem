//! Envelope-keyed DEK cache (TAN-666).
//!
//! Hosted memory encryption wraps a fresh per-scope data-encryption key (DEK)
//! with a KMS-held key-encryption key (KEK). Unwrapping a DEK is expensive — the
//! KMS provider spawns a subprocess and makes a KMS round-trip per call
//! ([`crate::kms_providers`]) — so a decrypt-heavy read path must cache the
//! unwrapped DEK. This module is that cache.
//!
//! **The cache key identifies a single envelope's DEK, not a scope.** It is
//! `(canonical_id, kek_version, rotation_epoch, wrapped_dek_fingerprint)`. Sealing
//! generates a fresh DEK per field, so two rows in the same scope and KEK version
//! carry *different* `wrapped_dek`s; keying by scope alone would let a later row's
//! DEK clobber an earlier row's cache entry and return the wrong key (AES-GCM
//! auth failure) — making ordinary multi-row scopes unreadable. Including the
//! wrapped-DEK fingerprint gives each envelope its own entry, so multiple rows per
//! scope and old/new rows through a rotation all resolve to the right DEK. The
//! scope segment is retained so a revocation can drop every entry for a scope at
//! once. Entries are LRU-evicted.
//!
//! (Keying by the wrapped DEK is also forward-compatible with a future per-scope
//! DEK registry — reusing one DEK across a scope's rows simply collapses to one
//! fingerprint, giving O(distinct scopes) KMS calls without a cache change.)
//!
//! DEK bytes are held in a [`SecretDek`] that zeroes its memory on drop, and are
//! handed out behind an `Arc` so a cache eviction never leaves a live copy behind
//! that outlives its zeroization.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Length of a memory DEK in bytes (AES-256).
pub const MEMORY_DEK_LEN: usize = 32;

/// Default number of distinct envelope keys to keep unwrapped in memory. Sized
/// for the low-cardinality steady state (tenant × department × data_class ×
/// source, times a small number of live key versions); LRU-evicted beyond this.
pub const DEFAULT_DEK_CACHE_CAPACITY: usize = 2048;

/// A 256-bit DEK whose bytes are zeroed when the last reference is dropped, so an
/// unwrapped key never lingers in freed heap memory.
pub struct SecretDek([u8; MEMORY_DEK_LEN]);

impl SecretDek {
    pub fn new(bytes: [u8; MEMORY_DEK_LEN]) -> Self {
        Self(bytes)
    }

    /// Borrow the raw key bytes for a single encrypt/decrypt operation. Callers
    /// must not retain the slice beyond the call.
    pub fn expose(&self) -> &[u8; MEMORY_DEK_LEN] {
        &self.0
    }
}

impl Drop for SecretDek {
    fn drop(&mut self) {
        // Best-effort zeroization. `write_volatile` in a loop is not reordered or
        // elided by the optimizer, which a plain `= [0; N]` could be.
        for byte in self.0.iter_mut() {
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
    }
}

impl std::fmt::Debug for SecretDek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render key material.
        f.write_str("SecretDek(***)")
    }
}

/// The cache key: a single envelope's DEK identity — the scope's `canonical_id`,
/// the KEK version and rotation epoch the row was sealed under, and a fingerprint
/// of the row's own `wrapped_dek`. The fingerprint is what lets two rows in the
/// same scope (each with a fresh DEK) cache independently; the `canonical_id` is
/// retained so an entire scope can be invalidated on revocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryDekCacheKey {
    pub canonical_id: String,
    pub kek_version: String,
    pub rotation_epoch: u64,
    pub wrapped_dek_fingerprint: String,
}

impl MemoryDekCacheKey {
    pub fn new(
        canonical_id: impl Into<String>,
        kek_version: impl Into<String>,
        rotation_epoch: u64,
        wrapped_dek_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            kek_version: kek_version.into(),
            rotation_epoch,
            wrapped_dek_fingerprint: wrapped_dek_fingerprint.into(),
        }
    }
}

struct CacheEntry {
    dek: Arc<SecretDek>,
    /// Monotonic access tick for LRU ordering (higher = more recently used).
    last_used: u64,
}

struct Inner {
    map: HashMap<MemoryDekCacheKey, CacheEntry>,
    tick: u64,
}

/// A concurrency-safe, LRU-bounded cache of unwrapped memory DEKs keyed by
/// [`MemoryDekCacheKey`] (scope + KEK version + rotation epoch + wrapped-DEK
/// fingerprint). Cheap to clone (`Arc` inside); share one across the read/write
/// path.
#[derive(Clone)]
pub struct MemoryDekCache {
    inner: Arc<Mutex<Inner>>,
    capacity: usize,
}

impl std::fmt::Debug for MemoryDekCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryDekCache")
            .field("capacity", &self.capacity)
            .field("len", &self.len())
            .finish()
    }
}

impl MemoryDekCache {
    /// Build a cache holding at most `capacity` unwrapped DEKs (minimum 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                map: HashMap::new(),
                tick: 0,
            })),
            capacity: capacity.max(1),
        }
    }

    /// A cache with [`DEFAULT_DEK_CACHE_CAPACITY`].
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_DEK_CACHE_CAPACITY)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Look up an unwrapped DEK, marking it most-recently-used on a hit.
    pub fn get(&self, key: &MemoryDekCacheKey) -> Option<Arc<SecretDek>> {
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let entry = inner.map.get_mut(key)?;
        entry.last_used = tick;
        Some(Arc::clone(&entry.dek))
    }

    /// Insert (or refresh) an unwrapped DEK for `key`, evicting the least-recently
    /// used entry first if the cache is at capacity. Returns the stored handle.
    pub fn insert(&self, key: MemoryDekCacheKey, dek: [u8; MEMORY_DEK_LEN]) -> Arc<SecretDek> {
        let handle = Arc::new(SecretDek::new(dek));
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        // Evict only when adding a genuinely new key would exceed capacity.
        if !inner.map.contains_key(&key) && inner.map.len() >= self.capacity {
            if let Some(evict_key) = inner
                .map
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(evict_key, _)| evict_key.clone())
            {
                inner.map.remove(&evict_key);
            }
        }
        inner.map.insert(
            key,
            CacheEntry {
                dek: Arc::clone(&handle),
                last_used: tick,
            },
        );
        handle
    }

    /// Drop every cached DEK for a scope (all key versions / rotation epochs).
    /// Called when a scope's keys are revoked so a revoked DEK cannot continue to
    /// decrypt from cache. Returns the number of entries dropped.
    pub fn invalidate_canonical_id(&self, canonical_id: &str) -> usize {
        let mut inner = self.lock();
        let before = inner.map.len();
        inner.map.retain(|key, _| key.canonical_id != canonical_id);
        before - inner.map.len()
    }

    /// Drop a single `(scope, version, epoch)` entry (e.g. one revoked version).
    pub fn invalidate_key(&self, key: &MemoryDekCacheKey) -> bool {
        self.lock().map.remove(key).is_some()
    }

    /// Drop every cached DEK (e.g. a global key-material rotation).
    pub fn clear(&self) {
        self.lock().map.clear();
    }

    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // Poisoning only happens if a holder panicked mid-mutation; the cache is
        // pure data, so recovering the guard is safe and preferable to a cascade.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for MemoryDekCache {
    fn default() -> Self {
        Self::with_default_capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dek(seed: u8) -> [u8; MEMORY_DEK_LEN] {
        [seed; MEMORY_DEK_LEN]
    }

    fn key(scope: &str, version: &str, epoch: u64, fingerprint: &str) -> MemoryDekCacheKey {
        MemoryDekCacheKey::new(scope, version, epoch, fingerprint)
    }

    #[test]
    fn hit_and_miss() {
        let cache = MemoryDekCache::new(8);
        let k = key("tandem/memory/acme/hq/prod/internal", "1", 0, "fp-a");
        assert!(cache.get(&k).is_none(), "cold miss");
        cache.insert(k.clone(), dek(7));
        assert_eq!(cache.get(&k).unwrap().expose(), &dek(7), "warm hit");
    }

    #[test]
    fn different_scopes_do_not_collide() {
        let cache = MemoryDekCache::new(8);
        let sales = key(
            "tandem/memory/acme/hq/prod/internal/dept/sales",
            "1",
            0,
            "fp-s",
        );
        let finance = key(
            "tandem/memory/acme/hq/prod/internal/dept/finance",
            "1",
            0,
            "fp-f",
        );
        cache.insert(sales.clone(), dek(1));
        cache.insert(finance.clone(), dek(2));
        assert_eq!(cache.get(&sales).unwrap().expose(), &dek(1));
        assert_eq!(cache.get(&finance).unwrap().expose(), &dek(2));
    }

    #[test]
    fn multiple_rows_in_one_scope_and_version_keep_distinct_deks() {
        // The regression this key design exists for: two rows in the same scope
        // and KEK version, each sealed with its own fresh DEK (distinct
        // wrapped_dek fingerprints). Sealing the second must NOT evict the first,
        // or reading the first row would decrypt with the wrong DEK.
        let cache = MemoryDekCache::new(8);
        let scope = "tandem/memory/acme/hq/prod/financial_record";
        let row_a = key(scope, "1", 0, "fp-row-a");
        let row_b = key(scope, "1", 0, "fp-row-b");
        cache.insert(row_a.clone(), dek(41));
        cache.insert(row_b.clone(), dek(42));
        assert_eq!(cache.get(&row_a).unwrap().expose(), &dek(41));
        assert_eq!(cache.get(&row_b).unwrap().expose(), &dek(42));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn key_versions_coexist_through_rotation() {
        // The same scope holds rows under kek_version 1 and 2 during a rotation;
        // both DEKs must be independently cached so neither row fails to decrypt.
        let cache = MemoryDekCache::new(8);
        let scope = "tandem/memory/acme/hq/prod/financial_record";
        let v1 = key(scope, "1", 0, "fp-v1");
        let v2 = key(scope, "2", 1, "fp-v2");
        cache.insert(v1.clone(), dek(11));
        cache.insert(v2.clone(), dek(22));
        assert_eq!(cache.get(&v1).unwrap().expose(), &dek(11));
        assert_eq!(cache.get(&v2).unwrap().expose(), &dek(22));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn invalidate_canonical_id_drops_all_entries_of_a_scope() {
        let cache = MemoryDekCache::new(8);
        let scope = "tandem/memory/acme/hq/prod/internal";
        let other = "tandem/memory/acme/hq/prod/confidential";
        cache.insert(key(scope, "1", 0, "fp-1"), dek(1));
        cache.insert(key(scope, "2", 1, "fp-2"), dek(2));
        cache.insert(key(other, "1", 0, "fp-1"), dek(3));
        let dropped = cache.invalidate_canonical_id(scope);
        assert_eq!(dropped, 2, "both entries of the revoked scope drop");
        assert!(cache.get(&key(scope, "1", 0, "fp-1")).is_none());
        assert!(cache.get(&key(scope, "2", 1, "fp-2")).is_none());
        assert!(
            cache.get(&key(other, "1", 0, "fp-1")).is_some(),
            "unrelated scope survives"
        );
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let cache = MemoryDekCache::new(2);
        let a = key("scope-a", "1", 0, "fp-a");
        let b = key("scope-b", "1", 0, "fp-b");
        let c = key("scope-c", "1", 0, "fp-c");
        cache.insert(a.clone(), dek(1));
        cache.insert(b.clone(), dek(2));
        // Touch `a` so `b` becomes the LRU victim.
        assert!(cache.get(&a).is_some());
        cache.insert(c.clone(), dek(3));
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&b).is_none(), "b was least-recently-used");
        assert!(cache.get(&a).is_some());
        assert!(cache.get(&c).is_some());
    }

    #[test]
    fn reinsert_same_key_does_not_evict() {
        let cache = MemoryDekCache::new(1);
        let k = key("scope", "1", 0, "fp-a");
        cache.insert(k.clone(), dek(1));
        cache.insert(k.clone(), dek(9));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&k).unwrap().expose(), &dek(9), "value refreshed");
    }

    #[test]
    fn secret_dek_never_renders_key_material() {
        let secret = SecretDek::new([0xABu8; MEMORY_DEK_LEN]);
        assert_eq!(format!("{secret:?}"), "SecretDek(***)");
        assert_eq!(secret.expose(), &[0xABu8; MEMORY_DEK_LEN]);
    }
}
