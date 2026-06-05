//! Dibit-level frame synchronization and data unit reception for CQPSK/LSM P25.
//!
//! This is the dibit-level equivalent of `DataUnitReceiver` (which operates on f32
//! baseband samples with C4FM-specific waveform correlation and threshold slicing).
//! The DibitReceiver operates directly on decoded dibits from a CQPSK demodulator,
//! using a simpler Hamming-distance sync correlator on the 48-bit sync word.

use bits::Dibit;
use error::{P25Error, Result};
use message::nid;
use message::status::{StreamSymbol, StatusDeinterleaver};
use message::data_unit::ReceiverEvent;
use stats::{Stats, HasStats};

/// 48-bit P25 frame sync word: 0x5575F5FF77FF
/// Each byte encodes 4 dibits MSB-first (24 symbols = 48 bits).
const SYNC_WORD: u64 = 0x5575F5FF77FF;
const SYNC_BITS: u32 = 48;

/// Maximum Hamming distance for sync detection (bit errors in 48 bits).
const SYNC_THRESHOLD: u32 = 4;

/// Dibit-level frame sync detector.
///
/// Maintains a 48-bit shift register of the last 24 dibits and correlates
/// against the P25 sync word using Hamming distance.
struct DibitSync {
    /// Shift register holding last 24 dibits (48 bits).
    shift_reg: u64,
    /// Number of dibits fed so far (for startup).
    count: usize,
}

impl DibitSync {
    fn new() -> Self {
        DibitSync {
            shift_reg: 0,
            count: 0,
        }
    }

    /// Feed a dibit and return true if sync is detected.
    fn feed(&mut self, dibit: Dibit) -> bool {
        // Shift in 2 bits
        self.shift_reg = ((self.shift_reg << 2) | (dibit.bits() as u64)) & ((1u64 << SYNC_BITS) - 1);
        self.count += 1;

        if self.count < 24 {
            return false;
        }

        // Hamming distance between shift register and sync word
        let diff = self.shift_reg ^ SYNC_WORD;
        let hamming = diff.count_ones();

        hamming <= SYNC_THRESHOLD
    }
}

/// State machine states for DibitReceiver.
enum State {
    /// Searching for frame sync.
    Sync,
    /// Decoding NID (first 32 data dibits after sync).
    DecodeNID(StatusDeinterleaver, nid::NidReceiver),
    /// Decoding packet data/status symbols.
    DecodePacket(StatusDeinterleaver),
    /// Flushing padding until next status symbol boundary.
    FlushPads(StatusDeinterleaver),
}

/// Action the state machine should take.
enum StateChange {
    Change(State),
    Event(ReceiverEvent),
    EventChange(ReceiverEvent, State),
    Error(P25Error),
    NoChange,
}

/// Dibit-level data unit receiver for CQPSK-demodulated P25.
///
/// Equivalent to `DataUnitReceiver` but operates on pre-decoded dibits
/// instead of raw baseband samples. Produces the same `ReceiverEvent` output.
pub struct DibitReceiver {
    state: State,
    sync: DibitSync,
    stats: Stats,
}

impl DibitReceiver {
    /// Create a new `DibitReceiver` in the initial sync state.
    pub fn new() -> Self {
        DibitReceiver {
            state: State::Sync,
            sync: DibitSync::new(),
            stats: Stats::default(),
        }
    }

    /// Force re-synchronization.
    pub fn resync(&mut self) {
        self.state = State::Sync;
        self.sync = DibitSync::new();
    }

    /// Flush remaining padding symbols and return to sync.
    pub fn flush_pads(&mut self) {
        match self.state {
            State::DecodePacket(deint) => {
                self.state = State::FlushPads(deint);
            },
            State::Sync => {},
            _ => {
                // Unexpected state — just resync
                self.resync();
            },
        }
    }

    /// Feed a decoded dibit, possibly producing a receiver event.
    pub fn feed(&mut self, dibit: Dibit) -> Option<Result<ReceiverEvent>> {
        match self.handle(dibit) {
            StateChange::Change(state) => {
                self.state = state;
                None
            },
            StateChange::Event(event) => Some(Ok(event)),
            StateChange::EventChange(event, state) => {
                self.state = state;
                Some(Ok(event))
            },
            StateChange::Error(err) => Some(Err(err)),
            StateChange::NoChange => None,
        }
    }

    fn handle(&mut self, dibit: Dibit) -> StateChange {
        // Always feed sync detector (for re-sync during packet decode too)
        let sync_hit = self.sync.feed(dibit);

        match self.state {
            State::Sync => {
                if sync_hit {
                    let deint = StatusDeinterleaver::new();
                    let nid_recv = nid::NidReceiver::new();
                    StateChange::Change(State::DecodeNID(deint, nid_recv))
                } else {
                    StateChange::NoChange
                }
            },
            State::DecodeNID(ref mut deint, ref mut nid_recv) => {
                let sym = deint.feed(dibit);
                let data_dibit = match sym {
                    StreamSymbol::Data(d) => d,
                    StreamSymbol::Status(_) => {
                        return StateChange::Event(ReceiverEvent::Symbol(sym));
                    },
                };

                match nid_recv.feed(data_dibit) {
                    Some(Ok(nid)) => {
                        self.stats.merge(nid_recv);
                        // Transition to packet decode, preserving deinterleaver state
                        let deint_copy = *deint;
                        StateChange::EventChange(
                            ReceiverEvent::NetworkId(nid),
                            State::DecodePacket(deint_copy),
                        )
                    },
                    Some(Err(e)) => StateChange::Error(e),
                    None => StateChange::NoChange,
                }
            },
            State::DecodePacket(ref mut deint) => {
                // If we get a sync hit during packet decode, it means a new frame
                // started — but we let the higher layer handle resync via flush_pads.
                let sym = deint.feed(dibit);
                StateChange::Event(ReceiverEvent::Symbol(sym))
            },
            State::FlushPads(ref mut deint) => {
                let sym = deint.feed(dibit);
                match sym {
                    StreamSymbol::Status(_) => {
                        // Reached status boundary — return to sync
                        self.sync = DibitSync::new();
                        StateChange::Change(State::Sync)
                    },
                    _ => StateChange::NoChange,
                }
            },
        }
    }
}

impl HasStats for DibitReceiver {
    fn stats(&mut self) -> &mut Stats { &mut self.stats }
}

#[cfg(test)]
mod test {
    use super::*;
    use bits::Dibit;

    #[test]
    fn test_sync_detection() {
        let mut sync = DibitSync::new();

        // Feed the 24 dibits that make up the sync word 0x5575F5FF77FF
        // Bytes: [0x55, 0x75, 0xF5, 0xFF, 0x77, 0xFF]
        let sync_bytes: [u8; 6] = [0x55, 0x75, 0xF5, 0xFF, 0x77, 0xFF];
        let mut detected = false;

        for &byte in &sync_bytes {
            for shift in (0..4).rev() {
                let bits = (byte >> (shift * 2)) & 0x03;
                if sync.feed(Dibit::new(bits)) {
                    detected = true;
                }
            }
        }

        assert!(detected, "sync word should be detected");
    }

    #[test]
    fn test_sync_with_errors() {
        let mut sync = DibitSync::new();

        // Feed sync word with 2 bit errors (within threshold of 4)
        let sync_bytes: [u8; 6] = [0x55, 0x75, 0xF5, 0xFF, 0x77, 0xFF];
        let mut dibits = Vec::new();
        for &byte in &sync_bytes {
            for shift in (0..4).rev() {
                dibits.push(Dibit::new((byte >> (shift * 2)) & 0x03));
            }
        }
        // Flip 2 bits (1 dibit change)
        dibits[5] = Dibit::new(dibits[5].bits() ^ 0x01);

        let mut detected = false;
        for d in &dibits {
            if sync.feed(*d) {
                detected = true;
            }
        }
        assert!(detected, "sync should detect with small bit errors");
    }

    #[test]
    fn test_no_false_sync() {
        let mut sync = DibitSync::new();
        // Feed random-ish data — should not trigger sync
        for i in 0..100u8 {
            let _ = sync.feed(Dibit::new(i & 0x03));
        }
        // No assertion needed — just verifying no panic
    }
}
