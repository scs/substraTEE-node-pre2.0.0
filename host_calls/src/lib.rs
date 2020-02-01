#[cfg(feature = "std")]
mod remote_attestation;

#[cfg(feature = "std")]
use remote_attestation::verify_mra_cert;

use runtime_interface::runtime_interface;
use codec::{Decode, Encode};

#[derive(Encode, Decode, Default, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct SgxReport {
    pub mr_enclave: [u8; 32],
    pub pubkey: [u8; 32]
}

#[runtime_interface]
pub trait RuntimeInterfaces {
	// Only types that implement the RIType (Runtime Interface Type) trait can be returned
	fn verify_ra_report(cert_der: &[u8], signer_attn: &[u32], signer: &[u8]) -> Option<Vec<u8>> {
		match verify_mra_cert(cert_der, signer_attn, signer) {
			Ok(rep) => Some(rep),
			Err(_) => None,
		}
	}
}
