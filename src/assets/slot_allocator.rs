use crate::config::MAX_FRAMES_IN_FLIGHT;

/// Manages a fixed pool of integer slots with deferred reclamation.
///
/// Slots are allocated immediately and freed only after `MAX_FRAMES_IN_FLIGHT`
/// frames have elapsed, ensuring no in-flight GPU frame references a reused slot.
pub struct SlotAllocator {
    free_slots: Vec<u32>,
    // (slot, frame after which it is safe to reuse)
    pending_reclaim: Vec<(u32, usize)>,
}

impl SlotAllocator {
    pub fn new(capacity: u32) -> Self {
        Self {
            free_slots: (0..capacity).rev().collect(),
            pending_reclaim: Vec::new(),
        }
    }

    /// Returns the next free slot, or `None` if the pool is exhausted.
    pub fn alloc(&mut self) -> Option<u32> {
        self.free_slots.pop()
    }

    /// Queues `slot` for return to the free pool after `MAX_FRAMES_IN_FLIGHT` frames.
    #[allow(dead_code)]
    pub fn free(&mut self, slot: u32, current_frame: usize) {
        self.pending_reclaim
            .push((slot, current_frame + MAX_FRAMES_IN_FLIGHT));
    }

    /// Moves slots whose deferred frame has passed back into the free pool.
    /// Call once per frame.
    pub fn flush(&mut self, current_frame: usize) {
        self.pending_reclaim.retain(|&(slot, safe_after)| {
            if current_frame >= safe_after {
                self.free_slots.push(slot);
                false
            } else {
                true
            }
        });
    }

    #[allow(dead_code)]
    pub fn free_count(&self) -> usize {
        self.free_slots.len()
    }

    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.pending_reclaim.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_fully_free() {
        let a = SlotAllocator::new(4);
        assert_eq!(a.free_count(), 4);
        assert_eq!(a.pending_count(), 0);
    }

    #[test]
    fn alloc_reduces_free_count() {
        let mut a = SlotAllocator::new(4);
        let s = a.alloc().unwrap();
        assert!(s < 4);
        assert_eq!(a.free_count(), 3);
    }

    #[test]
    fn alloc_returns_none_when_exhausted() {
        let mut a = SlotAllocator::new(2);
        a.alloc();
        a.alloc();
        assert!(a.alloc().is_none());
    }

    #[test]
    fn free_moves_to_pending_not_free() {
        let mut a = SlotAllocator::new(2);
        let slot = a.alloc().unwrap();
        a.free(slot, 0);
        assert_eq!(a.free_count(), 1); // only the other slot
        assert_eq!(a.pending_count(), 1);
    }

    #[test]
    fn flush_before_threshold_does_not_reclaim() {
        let mut a = SlotAllocator::new(2);
        let slot = a.alloc().unwrap();
        a.free(slot, 0);
        a.flush(MAX_FRAMES_IN_FLIGHT - 1);
        assert_eq!(a.pending_count(), 1);
        assert_eq!(a.free_count(), 1);
    }

    #[test]
    fn flush_at_threshold_reclaims_slot() {
        let mut a = SlotAllocator::new(2);
        let slot = a.alloc().unwrap();
        a.free(slot, 0);
        a.flush(MAX_FRAMES_IN_FLIGHT);
        assert_eq!(a.pending_count(), 0);
        assert_eq!(a.free_count(), 2);
    }

    #[test]
    fn reclaimed_slot_can_be_reallocated() {
        let mut a = SlotAllocator::new(1);
        let slot = a.alloc().unwrap();
        assert!(a.alloc().is_none());
        a.free(slot, 0);
        a.flush(MAX_FRAMES_IN_FLIGHT);
        assert!(a.alloc().is_some());
    }

    #[test]
    fn slots_are_unique() {
        let mut a = SlotAllocator::new(4);
        let mut seen = std::collections::HashSet::new();
        while let Some(s) = a.alloc() {
            assert!(seen.insert(s), "duplicate slot {s}");
        }
        assert_eq!(seen.len(), 4);
    }
}
