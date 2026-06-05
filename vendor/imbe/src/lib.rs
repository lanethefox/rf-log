//! Decode the Improved Multi-Band Excitation (IMBE) digital voice codec.

extern crate arrayvec;
extern crate collect_slice;
extern crate slice_mip;
extern crate iq_osc;
extern crate num_complex;
extern crate num_traits;
extern crate rand;
extern crate rand_distr;

pub mod allocs;
pub mod coefs;
pub mod consts;
pub mod decode;
pub mod descramble;
pub mod enhance;
pub mod frame;
pub mod gain;
pub mod params;
pub mod prev;
pub mod scan;
pub mod spectral;
pub mod unvoiced;
pub mod voiced;
pub mod window;

pub use decode::ImbeDecoder;
pub use frame::ReceivedFrame;
