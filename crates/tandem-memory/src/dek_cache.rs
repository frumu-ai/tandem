//! Envelope-keyed DEK cache (TAN-666).
//!
//! Hosted memory encryption wraps a fresh per-scope data-encryption key (DEK)
//! with a KMS-held key-encryption key (KEK). Unwrapping a DEK is expensive — the
//! KMS provider spawns a subprocess and makes a KMS round-trip per call
//! ([`crate::kms_providers`]) — so a decrypt-heavy read path must cache the
//! unwrapped DEK. This module is that cache.
//!
//! **The cache key is `(canonical_id, kek_version, rotation_epoch)`, not
//! `canonical_id` alone.** During rotation or backfill a single scope legitimately
//! holds rows sealed under different KEK versions / rotation epochs (each with its
//! own `wrapped_dek`). A scope-only cache would hand back the first-seen DEK for a
//! row sealed under a newer version and cause AES-GCM authentication failures.
//! Versioning the key lets old and new rows coexist through a rotation; entries
//! are LRU-evicted, and a whole scope's entries are dropped on revocation.
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

/// The cache key: an envelope's scope identity plus the exact key version and
/// rotation epoch the row was sealed under. Two rows in the same scope sealed
/// under different KEK versions map to different entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryDekCacheKey {
    pub canonical_id: String,
    pub kek_version: String,
    pub rotation_epoch: u64,
}

impl MemoryDekCacheKey {
    pub fn new(
        canonical_id: impl Into<String>,
        kek_version: impl Into<String>,
        rotation_epoch: u64,
    ) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            kek_version: kek_version.into(),
            rotation_epoch,
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
/// `(canonical_id, kek_version, rotation_epoch)`. Cheap to clone (`Arc` inside);
/// share one instance across the read/write path.
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

    #[test]
    fn hit_and_miss() {
        let cache = MemoryDekCache::new(8);
        let key = MemoryDekCacheKey::new("tandem/memory/acme/hq/prod/internal", "1", 0);
        assert!(cache.get(&key).is_none(), "cold miss");
        cache.insert(key.clone(), dek(7));
        assert_eq!(cache.get(&key).unwrap().expose(), &dek(7), "warm hit");
    }

    #[test]
    fn different_scopes_do_not_collide() {
        let cache = MemoryDekCache::new(8);
        let sales =
            MemoryDekCacheKey::new("tandem/memory/acme/hq/prod/internal/dept/sales", "1", 0);
        let finance =
            MemoryDekCacheKey::new("tandem/memory/acme/hq/prod/internal/dept/finance", "1", 0);
        cache.insert(sales.clone(), dek(1));
        cache.insert(finance.clone(), dek(2));
        assert_eq!(cache.get(&sales).unwrap().expose(), &dek(1));
        assert_eq!(cache.get(&finance).unwrap().expose(), &dek(2));
    }

    #[test]
    fn key_versions_coexist_through_rotation() {
        // The same scope holds rows under kek_version 1 and 2 during a rotation;
        // both DEKs must be independently cached so neither row fails to decrypt.
        let cache = MemoryDekCache::new(8);
        let scope = "tandem/memory/acme/hq/prod/financial_record";
        let v1 = MemoryDekCacheKey::new(scope, "1", 0);
        let v2 = MemoryDekCacheKey::new(scope, "2", 1);
        cache.insert(v1.clone(), dek(11));
        cache.insert(v2.clone(), dek(22));
        assert_eq!(cache.get(&v1).unwrap().expose(), &dek(11));
        assert_eq!(cache.get(&v2).unwrap().expose(), &dek(22));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn invalidate_canonical_id_drops_all_versions_of_a_scope() {
        let cache = MemoryDekCache::new(8);
        let scope = "tandem/memory/acme/hq/prod/internal";
        let other = "tandem/memory/acme/hq/prod/confidential";
        cache.insert(MemoryDekCacheKey::new(scope, "1", 0), dek(1));
        cache.insert(MemoryDekCacheKey::new(scope, "2", 1), dek(2));
        cache.insert(MemoryDekCacheKey::new(other, "1", 0), dek(3));
        let dropped = cache.invalidate_canonical_id(scope);
        assert_eq!(dropped, 2, "both versions of the revoked scope drop");
        assert!(cache.get(&MemoryDekCacheKey::new(scope, "1", 0)).is_none());
        assert!(cache.get(&MemoryDekCacheKey::new(scope, "2", 1)).is_none());
        assert!(
            cache.get(&MemoryDekCacheKey::new(other, "1", 0)).is_some(),
            "unrelated scope survives"
        );
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let cache = MemoryDekCache::new(2);
        let a = MemoryDekCacheKey::new("scope-a", "1", 0);
        let b = MemoryDekCacheKey::new("scope-b", "1", 0);
        let c = MemoryDekCacheKey::new("scope-c", "1", 0);
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
        let key = MemoryDekCacheKey::new("scope", "1", 0);
        cache.insert(key.clone(), dek(1));
        cache.insert(key.clone(), dek(9));
        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.get(&key).unwrap().expose(),
            &dek(9),
            "value refreshed"
        );
    }

    #[test]
    fn secret_dek_never_renders_key_material() {
        let secret = SecretDek::new([0xABu8; MEMORY_DEK_LEN]);
        assert_eq!(format!("{secret:?}"), "SecretDek(***)");
        assert_eq!(secret.expose(), &[0xABu8; MEMORY_DEK_LEN]);
    }
}
