//! IQ fan-out primitives with the two semantics the architecture requires
//! (`docs/RF-LOG-v2.md` §4.1):
//!
//! - [`LossyIqRing`] — overwrite-oldest. Correct for a survey/PSD tap that only
//!   wants "recent" samples; a slow consumer loses old data, never blocks the producer.
//!   One writer → N independent lossy rings gives lossless-free broadcast.
//! - [`LosslessIqRing`] — bounded, **never overwrites**. For a dwell collector that
//!   must not drop samples: when full it refuses the write and counts the drop, so
//!   loss is visible rather than silent (the v1 ring hid it).

use num_complex::Complex32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const ZERO: Complex32 = Complex32::new(0.0, 0.0);

struct Shared {
    buf: Box<[Complex32]>,
    capacity: usize,
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
    dropped: AtomicUsize,
}

// ---------------------------------------------------------------------------
// Lossy (overwrite-oldest) SPSC tap
// ---------------------------------------------------------------------------

pub struct LossyProducer {
    shared: Arc<Shared>,
}
pub struct LossyConsumer {
    shared: Arc<Shared>,
}
// SAFETY: producer and consumer each touch only their own cursor (atomically).
unsafe impl Send for LossyProducer {}
unsafe impl Send for LossyConsumer {}

/// Lossy overwrite-oldest ring. Capacity must be a power of two.
pub struct LossyIqRing;

impl LossyIqRing {
    #[allow(clippy::new_ret_no_self)] // intentionally returns (producer, consumer)
    pub fn new(capacity: usize) -> (LossyProducer, LossyConsumer) {
        assert!(
            capacity.is_power_of_two(),
            "capacity must be a power of two"
        );
        let shared = Arc::new(Shared {
            buf: vec![ZERO; capacity].into_boxed_slice(),
            capacity,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        });
        (
            LossyProducer {
                shared: shared.clone(),
            },
            LossyConsumer { shared },
        )
    }
}

impl LossyProducer {
    /// Push all samples, overwriting the oldest unread data if full.
    pub fn push(&self, data: &[Complex32]) {
        let s = &self.shared;
        let mask = s.capacity - 1;
        let mut write = s.write_pos.load(Ordering::Relaxed);
        for &sample in data {
            // SAFETY: single writer; idx always in bounds.
            unsafe {
                (s.buf.as_ptr() as *mut Complex32)
                    .add(write & mask)
                    .write(sample);
            }
            write = write.wrapping_add(1);
        }
        s.write_pos.store(write, Ordering::Release);
        let read = s.read_pos.load(Ordering::Acquire);
        if write.wrapping_sub(read) > s.capacity {
            s.read_pos
                .store(write.wrapping_sub(s.capacity), Ordering::Release);
        }
    }
}

impl LossyConsumer {
    /// Pop up to `out.len()` recent samples; returns the count read.
    pub fn pop(&self, out: &mut [Complex32]) -> usize {
        let s = &self.shared;
        let mask = s.capacity - 1;
        let write = s.write_pos.load(Ordering::Acquire);
        let mut read = s.read_pos.load(Ordering::Relaxed);
        let count = write.wrapping_sub(read).min(out.len());
        for slot in out.iter_mut().take(count) {
            *slot = s.buf[read & mask];
            read = read.wrapping_add(1);
        }
        s.read_pos.store(read, Ordering::Release);
        count
    }

    pub fn available(&self) -> usize {
        let s = &self.shared;
        s.write_pos
            .load(Ordering::Acquire)
            .wrapping_sub(s.read_pos.load(Ordering::Acquire))
    }
}

// ---------------------------------------------------------------------------
// Lossless (no-overwrite) SPSC collector
// ---------------------------------------------------------------------------

pub struct LosslessProducer {
    shared: Arc<Shared>,
}
pub struct LosslessConsumer {
    shared: Arc<Shared>,
}
unsafe impl Send for LosslessProducer {}
unsafe impl Send for LosslessConsumer {}

/// Bounded ring that never overwrites unread data. Capacity must be a power of two.
pub struct LosslessIqRing;

impl LosslessIqRing {
    #[allow(clippy::new_ret_no_self)] // intentionally returns (producer, consumer)
    pub fn new(capacity: usize) -> (LosslessProducer, LosslessConsumer) {
        assert!(
            capacity.is_power_of_two(),
            "capacity must be a power of two"
        );
        let shared = Arc::new(Shared {
            buf: vec![ZERO; capacity].into_boxed_slice(),
            capacity,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        });
        (
            LosslessProducer {
                shared: shared.clone(),
            },
            LosslessConsumer { shared },
        )
    }
}

impl LosslessProducer {
    /// Write as many samples as fit without overwriting unread data. Returns the
    /// number accepted; the remainder is counted as dropped (see [`dropped`]).
    ///
    /// [`dropped`]: LosslessConsumer::dropped
    pub fn push(&self, data: &[Complex32]) -> usize {
        let s = &self.shared;
        let mask = s.capacity - 1;
        let read = s.read_pos.load(Ordering::Acquire);
        let mut write = s.write_pos.load(Ordering::Relaxed);
        let free = s.capacity - write.wrapping_sub(read);
        let accept = free.min(data.len());
        for &sample in &data[..accept] {
            // SAFETY: single writer; within free space so never clobbers unread data.
            unsafe {
                (s.buf.as_ptr() as *mut Complex32)
                    .add(write & mask)
                    .write(sample);
            }
            write = write.wrapping_add(1);
        }
        s.write_pos.store(write, Ordering::Release);
        let dropped = data.len() - accept;
        if dropped > 0 {
            s.dropped.fetch_add(dropped, Ordering::Relaxed);
        }
        accept
    }
}

impl LosslessConsumer {
    pub fn pop(&self, out: &mut [Complex32]) -> usize {
        let s = &self.shared;
        let mask = s.capacity - 1;
        let write = s.write_pos.load(Ordering::Acquire);
        let mut read = s.read_pos.load(Ordering::Relaxed);
        let count = write.wrapping_sub(read).min(out.len());
        for slot in out.iter_mut().take(count) {
            *slot = s.buf[read & mask];
            read = read.wrapping_add(1);
        }
        s.read_pos.store(read, Ordering::Release);
        count
    }

    /// Total samples dropped because the ring was full — visible loss, by design.
    pub fn dropped(&self) -> usize {
        self.shared.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Vec<Complex32> {
        (0..n).map(|i| Complex32::new(i as f32, 0.0)).collect()
    }

    #[test]
    fn lossy_overwrites_oldest_when_full() {
        let (p, c) = LossyIqRing::new(4);
        p.push(&ramp(6)); // 0..6 into a cap-4 ring → keeps last 4
        let mut out = vec![ZERO; 8];
        let n = c.pop(&mut out);
        assert_eq!(n, 4);
        assert_eq!(out[0].re, 2.0);
        assert_eq!(out[3].re, 5.0);
    }

    #[test]
    fn lossless_refuses_and_counts_drops() {
        let (p, c) = LosslessIqRing::new(4);
        let accepted = p.push(&ramp(6)); // only 4 fit
        assert_eq!(accepted, 4);
        assert_eq!(c.dropped(), 2);
        let mut out = vec![ZERO; 8];
        let n = c.pop(&mut out);
        assert_eq!(n, 4);
        // oldest preserved (no overwrite)
        assert_eq!(out[0].re, 0.0);
        assert_eq!(out[3].re, 3.0);
    }

    #[test]
    fn lossless_drains_and_refills() {
        let (p, c) = LosslessIqRing::new(8);
        assert_eq!(p.push(&ramp(8)), 8);
        let mut out = vec![ZERO; 8];
        assert_eq!(c.pop(&mut out), 8);
        // space freed → next push accepted fully
        assert_eq!(p.push(&ramp(5)), 5);
        assert_eq!(c.dropped(), 0);
    }
}
