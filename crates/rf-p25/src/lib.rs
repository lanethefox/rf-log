//! rf-p25 — P25 Phase 1 voice decoder facade.
//!
//! Wraps kchmck's `p25` (message receiver) and `imbe` (voice codec) into a clean
//! single-call API: feed baseband samples → get decoded audio + metadata.

use p25::message::receiver::{MessageReceiver, MessageEvent};
use p25::trunking::tsbk::{TsbkFields, TsbkOpcode};
use p25::trunking::fields::{
    self as tfields,
    ChannelParamsUpdate as VendorChannelParamsUpdate,
    GroupTrafficUpdate,
};
use p25::voice::control::{self, LinkControlFields, LinkControlOpcode};
use p25::voice::crypto::{CryptoControlFields, CryptoAlgorithm};
use imbe::{ImbeDecoder, ReceivedFrame};
use serde::{Serialize, Deserialize};

/// Result of feeding one baseband sample to the P25 decoder.
pub enum P25Result {
    /// No output yet (accumulating symbols).
    None,
    /// Decoded voice audio: 160 samples at 8 kHz (one IMBE frame = 20 ms).
    Audio(Vec<f32>),
}

/// P25 link-layer metadata extracted from voice headers, link control, and crypto control.
#[derive(Clone, Debug, Serialize)]
pub struct P25Metadata {
    /// Network Access Code (12-bit).
    pub nac: u16,
    /// Data Unit ID string: "HDU", "LDU1", "LDU2", "TLC", "TSBK", etc.
    pub duid: String,
    /// Talkgroup ID (from Link Control).
    pub talkgroup: Option<u32>,
    /// Source radio unit ID (from Link Control).
    pub source_unit: Option<u32>,
    /// Whether encryption is active (algorithm != Unencrypted).
    pub encrypted: bool,
    /// Encryption algorithm name (from Crypto Control).
    pub algorithm: Option<String>,
    /// Encryption key ID (from Crypto Control).
    pub key_id: Option<u16>,
}

impl Default for P25Metadata {
    fn default() -> Self {
        P25Metadata {
            nac: 0,
            duid: String::new(),
            talkgroup: None,
            source_unit: None,
            encrypted: false,
            algorithm: None,
            key_id: None,
        }
    }
}

// ── TSBK (Trunking Signaling Block) types ──

/// Parsed TSBK payload with opcode metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TsbkPayload {
    /// Human-readable opcode name.
    pub opcode: String,
    /// Raw 6-bit opcode value.
    pub opcode_raw: u8,
    /// Whether the TSBK itself is encrypted.
    pub protected: bool,
    /// Whether CRC validated successfully.
    pub crc_valid: bool,
    /// Parsed payload data.
    pub payload: TsbkData,
}

/// Parsed TSBK payload variants by opcode family.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TsbkData {
    GroupVoiceGrant {
        talkgroup: u32,
        src_unit: u32,
        channel_id: u8,
        channel_num: u16,
        emergency: bool,
        encrypted: bool,
    },
    GroupVoiceUpdate {
        updates: Vec<TsbkGrantUpdate>,
    },
    UnitVoiceGrant {
        src_unit: u32,
        dest_unit: u32,
        channel_id: u8,
        channel_num: u16,
    },
    NetworkStatusBroadcast {
        wacn: u32,
        system: u16,
        channel_id: u8,
        channel_num: u16,
    },
    RfssStatusBroadcast {
        system: u16,
        rfss: u8,
        site: u8,
        channel_id: u8,
        channel_num: u16,
        networked: bool,
    },
    AdjacentSite {
        system: u16,
        rfss: u8,
        site: u8,
        channel_id: u8,
        channel_num: u16,
    },
    ChannelParamsUpdate {
        id: u8,
        bandwidth_hz: u32,
        base_freq_hz: u32,
        spacing_hz: u32,
    },
    AltControlChannel {
        rfss: u8,
        site: u8,
        channels: Vec<TsbkAltChannel>,
    },
    GroupAffiliation {
        talkgroup: u32,
        src_unit: u32,
    },
    UnitRegistration {
        system: u16,
        src_unit: u32,
    },
    UnitDeregistration {
        wacn: u32,
        system: u16,
        src_unit: u32,
    },
    DenyResponse {
        src_unit: u32,
    },
    Other {
        raw: Vec<u8>,
    },
}

/// A single voice grant update entry (channel + talkgroup pair).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TsbkGrantUpdate {
    pub channel_id: u8,
    pub channel_num: u16,
    pub talkgroup: u32,
}

/// An alternative control channel entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TsbkAltChannel {
    pub channel_id: u8,
    pub channel_num: u16,
}

/// P25 Phase 1 decoder: baseband → decoded voice + metadata.
///
/// Feed FM-demodulated baseband at 48 kHz. Internally runs the P25 symbol
/// synchronizer, error correction, and IMBE vocoder.
pub struct P25Decoder {
    receiver: MessageReceiver,
    vocoder: ImbeDecoder,
    metadata: P25Metadata,
    metadata_updated: bool,
    tsbk_events: Vec<TsbkPayload>,
    // Diagnostic counters for decode pipeline debugging
    pub sync_nid_count: u64,   // PacketNID events (sync found + NID decoded)
    pub decode_error_count: u64, // Error events (sync/NID/decode failures)
    pub duid_counts: [u64; 8], // Counts per DUID: [HDU, LDU1, LDU2, TLC, TSBK, PDU, SimpleTerm, Unknown]
}

impl P25Decoder {
    /// Create a new decoder in the initial state.
    pub fn new() -> Self {
        P25Decoder {
            receiver: MessageReceiver::new(),
            vocoder: ImbeDecoder::new(),
            metadata: P25Metadata::default(),
            metadata_updated: false,
            tsbk_events: Vec::new(),
            sync_nid_count: 0,
            decode_error_count: 0,
            duid_counts: [0; 8],
        }
    }

    /// Feed one FM-demodulated baseband sample (48 kHz) — C4FM path.
    ///
    /// Returns `P25Result::Audio(samples)` when a voice frame completes
    /// (160 samples at 8 kHz, normalized to ±1.0). Returns `P25Result::None` otherwise.
    pub fn feed(&mut self, sample: f32) -> P25Result {
        let event = match self.receiver.feed(sample) {
            Some(e) => e,
            None => return P25Result::None,
        };

        self.process_event(event)
    }

    /// Feed one decoded dibit — CQPSK/LSM path.
    ///
    /// Same output as `feed()` but takes pre-decoded dibits from a CQPSK demodulator
    /// instead of FM-demodulated baseband samples.
    pub fn feed_dibit(&mut self, dibit: p25::bits::Dibit) -> P25Result {
        let event = match self.receiver.feed_dibit(dibit) {
            Some(e) => e,
            None => return P25Result::None,
        };

        self.process_event(event)
    }

    /// Process a MessageEvent from either the C4FM or CQPSK path.
    fn process_event(&mut self, event: MessageEvent) -> P25Result {
        match event {
            MessageEvent::VoiceFrame(vf) => {
                let frame = ReceivedFrame {
                    chunks: vf.chunks,
                    errors: vf.errors,
                };
                let mut buf = [0.0f32; 160];
                self.vocoder.decode(frame, &mut buf);

                // Normalize IMBE output (raw values ~±8192) to ±1.0.
                let audio: Vec<f32> = buf.iter().map(|&s| s / 8192.0).collect();
                P25Result::Audio(audio)
            }

            MessageEvent::PacketNID(nid) => {
                self.sync_nid_count += 1;
                // Track DUID distribution: [HDU, SimpleTerm, LCTerm, LCFG, CCFG, DataPkt, TSBK, Other]
                let duid_idx = match nid.data_unit {
                    p25::message::nid::DataUnit::VoiceHeader => 0,
                    p25::message::nid::DataUnit::VoiceSimpleTerminator => 1,
                    p25::message::nid::DataUnit::VoiceLCTerminator => 2,
                    p25::message::nid::DataUnit::VoiceLCFrameGroup => 3,
                    p25::message::nid::DataUnit::VoiceCCFrameGroup => 4,
                    p25::message::nid::DataUnit::DataPacket => 5,
                    p25::message::nid::DataUnit::TrunkingSignaling => 6,
                };
                if duid_idx < self.duid_counts.len() {
                    self.duid_counts[duid_idx] += 1;
                }
                self.metadata.nac = nid.access_code.to_bits();
                self.metadata.duid = format_duid(&nid.data_unit);
                self.metadata_updated = true;
                P25Result::None
            }

            MessageEvent::VoiceHeader(hdr) => {
                // VoiceHeaderFields is a tuple struct — extract via accessor methods.
                self.metadata.talkgroup = Some(talkgroup_to_u32(hdr.talk_group()));
                let alg = hdr.crypto_alg();
                self.metadata.encrypted = alg != CryptoAlgorithm::Unencrypted;
                self.metadata.algorithm = Some(format_algorithm(&alg));
                self.metadata.key_id = Some(hdr.crypto_key());
                self.metadata.duid = "HDU".into();
                self.metadata_updated = true;
                P25Result::None
            }

            MessageEvent::LinkControl(lc) => {
                self.extract_link_control(&lc);
                self.metadata_updated = true;
                P25Result::None
            }

            MessageEvent::CryptoControl(cc) => {
                self.extract_crypto_control(&cc);
                self.metadata_updated = true;
                P25Result::None
            }

            MessageEvent::VoiceTerm(_lc) => {
                self.metadata.duid = "TLC".into();
                self.metadata_updated = true;
                P25Result::None
            }

            MessageEvent::TrunkingControl(tsbk) => {
                self.metadata.duid = "TSBK".into();
                self.metadata_updated = true;
                self.parse_tsbk(tsbk);
                P25Result::None
            }

            MessageEvent::LowSpeedDataFragment(_) => P25Result::None,
            MessageEvent::Error(_) => {
                self.decode_error_count += 1;
                P25Result::None
            }
        }
    }

    /// Take the latest metadata if it has been updated since the last call.
    pub fn take_metadata(&mut self) -> Option<P25Metadata> {
        if self.metadata_updated {
            self.metadata_updated = false;
            Some(self.metadata.clone())
        } else {
            None
        }
    }

    /// Take any queued TSBK events (control channel data).
    pub fn take_tsbk_events(&mut self) -> Vec<TsbkPayload> {
        std::mem::take(&mut self.tsbk_events)
    }

    /// Peek at how many TSBK events are queued (for diagnostics).
    pub fn peek_tsbk_count(&self) -> usize {
        self.tsbk_events.len()
    }

    /// Reset decoder state (call on channel change).
    pub fn reset(&mut self) {
        self.receiver = MessageReceiver::new();
        self.vocoder = ImbeDecoder::new();
        self.metadata = P25Metadata::default();
        self.metadata_updated = false;
        self.tsbk_events.clear();
    }

    /// Force frame re-synchronization (call on frequency change).
    /// Lighter than reset() — keeps vocoder state but re-acquires frame sync.
    pub fn resync(&mut self) {
        self.receiver.resync();
    }

    /// Parse a TSBK packet and push to the event queue.
    fn parse_tsbk(&mut self, tsbk: TsbkFields) {
        let crc_valid = tsbk.crc_valid();
        let protected = tsbk.protected();

        // Log non-standard manufacturer TSBKs but still process standard opcode
        let mfg = tsbk.mfg();
        if mfg != 0 {
            tracing::info!(
                "TSBK vendor mfg=0x{:02X} opcode_raw=0x{:02X} crc={} — skipping",
                mfg, tsbk.raw_opcode(), crc_valid
            );
            return;
        }

        let raw_op = tsbk.raw_opcode();
        let opcode = match tsbk.opcode() {
            Some(op) => op,
            None => {
                tracing::info!("TSBK unknown opcode raw=0x{:02X} crc={}", raw_op, crc_valid);
                return;
            }
        };

        let opcode_name = format_tsbk_opcode(&opcode);
        let opcode_bits = tsbk_opcode_to_raw(&opcode);

        let payload = match opcode {
            TsbkOpcode::GroupVoiceGrant => {
                let g = p25::trunking::tsbk::GroupVoiceGrant::new(tsbk);
                let ch = g.channel();
                TsbkData::GroupVoiceGrant {
                    talkgroup: talkgroup_to_u32(g.talkgroup()),
                    src_unit: g.src_unit(),
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                    emergency: g.opts().emergency(),
                    encrypted: g.opts().protected(),
                }
            }

            TsbkOpcode::GroupVoiceUpdate | TsbkOpcode::GroupDataUpdate => {
                let u = GroupTrafficUpdate::new(tsbk.payload());
                let pairs = u.updates();
                let updates = pairs.iter().map(|&(ch, tg)| TsbkGrantUpdate {
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                    talkgroup: talkgroup_to_u32(tg),
                }).collect();
                TsbkData::GroupVoiceUpdate { updates }
            }

            TsbkOpcode::UnitVoiceGrant | TsbkOpcode::UnitVoiceUpdate | TsbkOpcode::UnitDataGrant => {
                let g = p25::trunking::tsbk::UnitTrafficChannel::new(tsbk);
                let ch = g.channel();
                TsbkData::UnitVoiceGrant {
                    src_unit: g.src_unit(),
                    dest_unit: g.dest_unit(),
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                }
            }

            TsbkOpcode::NetworkStatusBroadcast => {
                let n = tfields::NetworkStatusBroadcast::new(tsbk.payload());
                let ch = n.channel();
                TsbkData::NetworkStatusBroadcast {
                    wacn: n.wacn(),
                    system: n.system(),
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                }
            }

            TsbkOpcode::RfssStatusBroadcast => {
                let r = tfields::RfssStatusBroadcast::new(tsbk.payload());
                let ch = r.channel();
                TsbkData::RfssStatusBroadcast {
                    system: r.system(),
                    rfss: r.rfss(),
                    site: r.site(),
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                    networked: r.networked(),
                }
            }

            TsbkOpcode::AdjacentSite => {
                let a = tfields::AdjacentSite::new(tsbk.payload());
                let ch = a.channel();
                TsbkData::AdjacentSite {
                    system: a.system(),
                    rfss: a.rfss(),
                    site: a.site(),
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                }
            }

            TsbkOpcode::ChannelParamsUpdate => {
                let p = VendorChannelParamsUpdate::new(tsbk.payload());
                let params = p.params();
                TsbkData::ChannelParamsUpdate {
                    id: p.id(),
                    bandwidth_hz: params.bandwidth,
                    // Extract base_freq and spacing from the ChannelParams
                    // rx_freq(0) gives the base frequency
                    base_freq_hz: params.rx_freq(0),
                    spacing_hz: if params.rx_freq(1) > params.rx_freq(0) {
                        params.rx_freq(1) - params.rx_freq(0)
                    } else {
                        0
                    },
                }
            }

            TsbkOpcode::AltControlChannel => {
                let a = tfields::AltControlChannel::new(tsbk.payload());
                let alts = a.alts();
                let channels = alts.iter().map(|&(ch, _svc)| TsbkAltChannel {
                    channel_id: ch.id(),
                    channel_num: ch.number(),
                }).collect();
                TsbkData::AltControlChannel {
                    rfss: a.rfss(),
                    site: a.site(),
                    channels,
                }
            }

            TsbkOpcode::GroupAffiliationResponse => {
                // Parse raw payload: [reserved, tg_hi, tg_lo, ann_hi, ann_lo, src2, src1, src0]
                let pl = tsbk.payload();
                let tg = ((pl[1] as u32) << 8) | pl[2] as u32;
                let src = ((pl[5] as u32) << 16) | ((pl[6] as u32) << 8) | pl[7] as u32;
                TsbkData::GroupAffiliation {
                    talkgroup: tg,
                    src_unit: src,
                }
            }

            TsbkOpcode::UnitRegResponse => {
                let r = p25::trunking::tsbk::UnitRegResponse::new(tsbk);
                TsbkData::UnitRegistration {
                    system: r.system(),
                    src_unit: r.src_addr(),
                }
            }

            TsbkOpcode::UnitDeregAck => {
                let a = p25::trunking::tsbk::UnitDeregAck::new(tsbk);
                TsbkData::UnitDeregistration {
                    wacn: a.wacn(),
                    system: a.system(),
                    src_unit: a.src_unit(),
                }
            }

            TsbkOpcode::DenyResponse => {
                // Parse raw payload: [info, svc_type, tgt2, tgt1, tgt0, src2, src1, src0]
                let pl = tsbk.payload();
                let src = ((pl[5] as u32) << 16) | ((pl[6] as u32) << 8) | pl[7] as u32;
                TsbkData::DenyResponse { src_unit: src }
            }

            _ => {
                TsbkData::Other { raw: tsbk.payload().to_vec() }
            }
        };

        self.tsbk_events.push(TsbkPayload {
            opcode: opcode_name,
            opcode_raw: opcode_bits,
            protected,
            crc_valid,
            payload,
        });
    }

    fn extract_link_control(&mut self, lc: &LinkControlFields) {
        match lc.opcode() {
            Some(LinkControlOpcode::GroupVoiceTraffic) => {
                let gvt = control::GroupVoiceTraffic::new(*lc);
                let tg_raw = gvt.talkgroup();
                self.metadata.talkgroup = Some(talkgroup_to_u32(tg_raw));
                self.metadata.source_unit = Some(gvt.src_unit());
                self.metadata.encrypted = gvt.opts().protected();
            }
            Some(LinkControlOpcode::UnitVoiceTraffic) => {
                let uvt = control::UnitVoiceTraffic::new(*lc);
                self.metadata.source_unit = Some(uvt.src_unit());
                self.metadata.encrypted = uvt.opts().protected();
            }
            _ => {}
        }
    }

    fn extract_crypto_control(&mut self, cc: &CryptoControlFields) {
        let alg = cc.alg();
        self.metadata.encrypted = alg != CryptoAlgorithm::Unencrypted;
        self.metadata.algorithm = Some(format_algorithm(&alg));
        self.metadata.key_id = Some(cc.key());
    }
}

fn format_duid(du: &p25::message::nid::DataUnit) -> String {
    use p25::message::nid::DataUnit::*;
    match *du {
        VoiceHeader => "HDU".into(),
        VoiceLCFrameGroup => "LDU1".into(),
        VoiceCCFrameGroup => "LDU2".into(),
        VoiceLCTerminator => "TLC".into(),
        VoiceSimpleTerminator => "VSELP".into(),
        DataPacket => "PDU".into(),
        TrunkingSignaling => "TSBK".into(),
    }
}

fn format_algorithm(alg: &CryptoAlgorithm) -> String {
    match *alg {
        CryptoAlgorithm::Unencrypted => "Unencrypted".into(),
        CryptoAlgorithm::Des => "DES-OFB".into(),
        CryptoAlgorithm::TripleDes => "3DES".into(),
        CryptoAlgorithm::Aes => "AES-256".into(),
        CryptoAlgorithm::Accordion => "Accordion".into(),
        CryptoAlgorithm::BatonEven => "Baton (Even)".into(),
        CryptoAlgorithm::BatonOdd => "Baton (Odd)".into(),
        CryptoAlgorithm::Firefly => "Firefly".into(),
        CryptoAlgorithm::Mayfly => "Mayfly".into(),
        CryptoAlgorithm::Saville => "Saville".into(),
        CryptoAlgorithm::Other(id) => format!("Unknown(0x{:02X})", id),
    }
}

fn talkgroup_to_u32(tg: p25::trunking::fields::TalkGroup) -> u32 {
    use p25::trunking::fields::TalkGroup;
    match tg {
        TalkGroup::Everbody => 0xFFFF,
        TalkGroup::Nobody => 0,
        TalkGroup::Default => 1,
        TalkGroup::Other(v) => v as u32,
    }
}

fn format_tsbk_opcode(op: &TsbkOpcode) -> String {
    match op {
        TsbkOpcode::GroupVoiceGrant => "GroupVoiceGrant",
        TsbkOpcode::GroupVoiceUpdate => "GroupVoiceUpdate",
        TsbkOpcode::GroupVoiceUpdateExplicit => "GroupVoiceUpdateExplicit",
        TsbkOpcode::UnitVoiceGrant => "UnitVoiceGrant",
        TsbkOpcode::UnitCallRequest => "UnitCallRequest",
        TsbkOpcode::UnitVoiceUpdate => "UnitVoiceUpdate",
        TsbkOpcode::PhoneGrant => "PhoneGrant",
        TsbkOpcode::PhoneAlert => "PhoneAlert",
        TsbkOpcode::UnitDataGrant => "UnitDataGrant",
        TsbkOpcode::GroupDataGrant => "GroupDataGrant",
        TsbkOpcode::GroupDataUpdate => "GroupDataUpdate",
        TsbkOpcode::GroupDataUpdateExplicit => "GroupDataUpdateExplicit",
        TsbkOpcode::UnitStatusUpdate => "UnitStatusUpdate",
        TsbkOpcode::UnitStatusQuery => "UnitStatusQuery",
        TsbkOpcode::UnitShortMessage => "UnitShortMessage",
        TsbkOpcode::UnitMonitor => "UnitMonitor",
        TsbkOpcode::UnitCallAlert => "UnitCallAlert",
        TsbkOpcode::AckResponse => "AckResponse",
        TsbkOpcode::QueuedResponse => "QueuedResponse",
        TsbkOpcode::ExtendedFunctionResponse => "ExtendedFunctionResponse",
        TsbkOpcode::DenyResponse => "DenyResponse",
        TsbkOpcode::GroupAffiliationResponse => "GroupAffiliationResponse",
        TsbkOpcode::GroupAffiliationQuery => "GroupAffiliationQuery",
        TsbkOpcode::LocRegResponse => "LocRegResponse",
        TsbkOpcode::UnitRegResponse => "UnitRegResponse",
        TsbkOpcode::UnitRegCommand => "UnitRegCommand",
        TsbkOpcode::UnitAuthCommand => "UnitAuthCommand",
        TsbkOpcode::UnitDeregAck => "UnitDeregAck",
        TsbkOpcode::RoamingAddrCommand => "RoamingAddrCommand",
        TsbkOpcode::RoamingAddrUpdate => "RoamingAddrUpdate",
        TsbkOpcode::SystemServiceBroadcast => "SystemServiceBroadcast",
        TsbkOpcode::AltControlChannel => "AltControlChannel",
        TsbkOpcode::RfssStatusBroadcast => "RfssStatusBroadcast",
        TsbkOpcode::NetworkStatusBroadcast => "NetworkStatusBroadcast",
        TsbkOpcode::AdjacentSite => "AdjacentSite",
        TsbkOpcode::ChannelParamsUpdate => "ChannelParamsUpdate",
        TsbkOpcode::ProtectionParamBroadcast => "ProtectionParamBroadcast",
        TsbkOpcode::ProtectionParamUpdate => "ProtectionParamUpdate",
        TsbkOpcode::Reserved => "Reserved",
    }.into()
}

fn tsbk_opcode_to_raw(op: &TsbkOpcode) -> u8 {
    match op {
        TsbkOpcode::GroupVoiceGrant => 0x00,
        TsbkOpcode::GroupVoiceUpdate => 0x02,
        TsbkOpcode::GroupVoiceUpdateExplicit => 0x03,
        TsbkOpcode::UnitVoiceGrant => 0x04,
        TsbkOpcode::UnitCallRequest => 0x05,
        TsbkOpcode::UnitVoiceUpdate => 0x06,
        TsbkOpcode::PhoneGrant => 0x08,
        TsbkOpcode::PhoneAlert => 0x0A,
        TsbkOpcode::UnitDataGrant => 0x10,
        TsbkOpcode::GroupDataGrant => 0x11,
        TsbkOpcode::GroupDataUpdate => 0x12,
        TsbkOpcode::GroupDataUpdateExplicit => 0x13,
        TsbkOpcode::UnitStatusUpdate => 0x18,
        TsbkOpcode::UnitStatusQuery => 0x1A,
        TsbkOpcode::UnitShortMessage => 0x1C,
        TsbkOpcode::UnitMonitor => 0x1D,
        TsbkOpcode::UnitCallAlert => 0x1F,
        TsbkOpcode::AckResponse => 0x20,
        TsbkOpcode::QueuedResponse => 0x21,
        TsbkOpcode::ExtendedFunctionResponse => 0x24,
        TsbkOpcode::DenyResponse => 0x27,
        TsbkOpcode::GroupAffiliationResponse => 0x28,
        TsbkOpcode::GroupAffiliationQuery => 0x2A,
        TsbkOpcode::LocRegResponse => 0x2B,
        TsbkOpcode::UnitRegResponse => 0x2C,
        TsbkOpcode::UnitRegCommand => 0x2D,
        TsbkOpcode::UnitAuthCommand => 0x2E,
        TsbkOpcode::UnitDeregAck => 0x2F,
        TsbkOpcode::RoamingAddrCommand => 0x36,
        TsbkOpcode::RoamingAddrUpdate => 0x37,
        TsbkOpcode::SystemServiceBroadcast => 0x38,
        TsbkOpcode::AltControlChannel => 0x39,
        TsbkOpcode::RfssStatusBroadcast => 0x3A,
        TsbkOpcode::NetworkStatusBroadcast => 0x3B,
        TsbkOpcode::AdjacentSite => 0x3C,
        TsbkOpcode::ChannelParamsUpdate => 0x3D,
        TsbkOpcode::ProtectionParamBroadcast => 0x3E,
        TsbkOpcode::ProtectionParamUpdate => 0x3F,
        TsbkOpcode::Reserved => 0x01,
    }
}
