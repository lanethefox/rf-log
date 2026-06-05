use crate::IqSample;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Lock-free SPSC ring buffer for IQ samples.
pub struct IqRingBuffer;

struct Shared {
    buf: Box<[IqSample]>,
    capacity: usize,
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
}

pub struct IqProducer {
    shared: Arc<Shared>,
}

pub struct IqConsumer {
    shared: Arc<Shared>,
}

// Safety: the producer and consumer each only access their own cursor atomically
unsafe impl Send for IqProducer {}
unsafe impl Send for IqConsumer {}

impl IqRingBuffer {
    /// Create a new SPSC ring buffer with the given capacity (must be power of 2).
    /// Returns (producer, consumer).
    pub fn new(capacity: usize) -> (IqProducer, IqConsumer) {
        assert!(capacity.is_power_of_two(), "capacity must be power of 2");
        let buf = vec![IqSample::new(0.0, 0.0); capacity].into_boxed_slice();
        let shared = Arc::new(Shared {
            buf,
            capacity,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
        });
        (
            IqProducer { shared: shared.clone() },
            IqConsumer { shared },
        )
    }
}

impl IqProducer {
    /// Push a slice of samples into the ring buffer.
    /// Returns the number of samples actually written.
    /// Overwrites oldest data if buffer is full.
    pub fn push_slice(&self, data: &[IqSample]) -> usize {
        let s = &self.shared;
        let mask = s.capacity - 1;
        let mut write = s.write_pos.load(Ordering::Relaxed);

        for &sample in data {
            let idx = write & mask;
            // Safety: we're the only writer, and idx is always in bounds
            unsafe {
                let ptr = s.buf.as_ptr() as *mut IqSample;
                ptr.add(idx).write(sample);
            }
            write = write.wrapping_add(1);
        }

        s.write_pos.store(write, Ordering::Release);

        // If we overwrote unread data, advance the read pointer
        let read = s.read_pos.load(Ordering::Acquire);
        if write.wrapping_sub(read) > s.capacity {
            s.read_pos.store(write.wrapping_sub(s.capacity), Ordering::Release);
        }

        data.len()
    }
}

impl IqConsumer {
    /// Pop up to `out.len()` samples from the ring buffer.
    /// Returns the number of samples actually read.
    pub fn pop_slice(&self, out: &mut [IqSample]) -> usize {
        let s = &self.shared;
        let mask = s.capacity - 1;
        let write = s.write_pos.load(Ordering::Acquire);
        let mut read = s.read_pos.load(Ordering::Relaxed);

        let available = write.wrapping_sub(read);
        let count = available.min(out.len());

        for item in out.iter_mut().take(count) {
            let idx = read & mask;
            *item = s.buf[idx];
            read = read.wrapping_add(1);
        }

        s.read_pos.store(read, Ordering::Release);
        count
    }

    /// Returns the number of samples currently available to read.
    pub fn available(&self) -> usize {
        let s = &self.shared;
        let write = s.write_pos.load(Ordering::Acquire);
        let read = s.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex32;

    #[test]
    fn basic_push_pop() {
        let (prod, cons) = IqRingBuffer::new(16);
        let data: Vec<IqSample> = (0..8).map(|i| Complex32::new(i as f32, 0.0)).collect();
        prod.push_slice(&data);

        assert_eq!(cons.available(), 8);
        let mut out = vec![Complex32::new(0.0, 0.0); 8];
        let n = cons.pop_slice(&mut out);
        assert_eq!(n, 8);
        assert_eq!(out[0].re, 0.0);
        assert_eq!(out[7].re, 7.0);
    }

    #[test]
    fn overwrite_on_full() {
        let (prod, cons) = IqRingBuffer::new(4);
        let data: Vec<IqSample> = (0..6).map(|i| Complex32::new(i as f32, 0.0)).collect();
        prod.push_slice(&data);

        // Should only have last 4 samples
        let mut out = vec![Complex32::new(0.0, 0.0); 8];
        let n = cons.pop_slice(&mut out);
        assert_eq!(n, 4);
        assert_eq!(out[0].re, 2.0);
        assert_eq!(out[3].re, 5.0);
    }
}
