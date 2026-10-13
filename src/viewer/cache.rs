// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `viewer::cache`: shared eviction helper for the viewer's bounded
//! per-frame caches.
//!
//! The code-block cache (`viewer::code`) and the table-width cache
//! (`viewer::table`) both evict least-recently-used entries under a slot
//! AND a byte budget in one sorted pass (the old per-victim `min_by_key`
//! rescan was O(cap²) inside a single frame when a new document generation
//! refilled a full cache). The loop is shared here so the two budgets
//! cannot drift apart; byte accounting stays at the call sites (a code
//! block's held bytes include its laid-out galleys, a table's do not).

use std::hash::Hash;

/// Evicts least-recently-used entries until `slots` more entries and
/// `bytes_deficit` more bytes have been freed.
///
/// - `evict`: called oldest-first with each candidate key; returns the bytes
///   that candidate held (0 when the key is already gone), so the caller
///   keeps its own byte total in lockstep.
/// - `aged`: `(last_use, key)` pairs, unsorted; sorted here, oldest first.
///
/// Pure logic over the caller's map: no behavior change at either site.
pub(super) fn evict_oldest<K, F>(
    aged: Vec<(u64, K)>,
    mut slots: usize,
    bytes_deficit: usize,
    mut evict: F,
) where
    K: Copy + Eq + Hash,
    F: FnMut(K) -> Option<usize>,
{
    if slots == 0 && bytes_deficit == 0 {
        return;
    }
    let mut by_age = aged;
    by_age.sort_unstable_by_key(|(used, _)| *used);
    let mut freed = 0usize;
    for (_, key) in by_age {
        if slots == 0 && freed >= bytes_deficit {
            break;
        }
        if let Some(held) = evict(key) {
            freed = freed.saturating_add(held);
            slots = slots.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::evict_oldest;
    use std::collections::HashMap;

    #[test]
    fn frees_slots_oldest_first() {
        let mut map: HashMap<u32, (u64, usize)> = [(1, (10, 100)), (2, (5, 100)), (3, (20, 100))]
            .into_iter()
            .collect();
        let aged: Vec<(u64, u32)> = map.iter().map(|(k, (u, _))| (*u, *k)).collect();
        evict_oldest(aged, 2, 0, |k| map.remove(&k).map(|(_, b)| b));
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&3), "newest must survive");
    }

    #[test]
    fn frees_bytes_until_deficit_covered() {
        let mut map: HashMap<u32, (u64, usize)> = [(1, (1, 100)), (2, (2, 500)), (3, (3, 100))]
            .into_iter()
            .collect();
        let aged: Vec<(u64, u32)> = map.iter().map(|(k, (u, _))| (*u, *k)).collect();
        evict_oldest(aged, 0, 550, |k| map.remove(&k).map(|(_, b)| b));
        assert!(!map.contains_key(&1));
        assert!(!map.contains_key(&2), "deficit needs both oldest");
        assert!(map.contains_key(&3));
    }

    #[test]
    fn no_work_when_budgets_hold() {
        let mut map: HashMap<u32, (u64, usize)> = [(1, (1, 100))].into_iter().collect();
        let mut calls = 0;
        evict_oldest(vec![(1, 1)], 0, 0, |k| {
            calls += 1;
            map.remove(&k).map(|(_, b)| b)
        });
        assert_eq!(calls, 0);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn missing_keys_do_not_spend_slots() {
        let mut map: HashMap<u32, (u64, usize)> = [(2, (5, 100))].into_iter().collect();
        // Key 1 is already gone: it must not consume a slot.
        evict_oldest(vec![(1, 1), (5, 2)], 1, 0, |k| {
            map.remove(&k).map(|(_, b)| b)
        });
        assert!(map.is_empty(), "slot must free the live entry");
    }
}
