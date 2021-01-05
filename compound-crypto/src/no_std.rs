use codec::{Decode, Encode};
use secp256k1::PublicKey;
use tiny_keccak::Hasher;

/// The crypto error type allows for various failure scenarios
///
/// * The key id provided is unknown
/// * The HSM is not available
/// * The HSM failed to sign this request for some other reason
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Encode, Decode)]
pub enum CryptoError {
    Unknown,
    KeyNotFound,
    KeyringLock,
    InvalidKeyId,
    ParseError,
    RecoverError,
    HSMError,
}

/// The default key id for the eth authority key (l1)
pub const ETH_KEY_ID_ENV_VAR_DEV_DEFAULT: &str = "my_eth_key_id";

/// For compatibility this is required.
pub(crate) const ETH_MESSAGE_PREAMBLE: &[u8] = "\x19Ethereum Signed Message:\n".as_bytes();
/// For compatibility this is required.
pub const ETH_ADD_TO_V: u8 = 27u8;

/// Helper function to quickly run keccak in the Ethereum-style
pub(crate) fn eth_keccak_for_signature(input: &[u8]) -> [u8; 32] {
    let mut output = [0u8; 32];
    let mut hasher = tiny_keccak::Keccak::v256();
    hasher.update(ETH_MESSAGE_PREAMBLE);
    hasher.update(format!("{}", input.len()).as_bytes());
    hasher.update(input);
    hasher.finalize(&mut output);
    output
}

pub fn keccak(input: &[u8]) -> [u8; 32] {
    let mut output = [0u8; 32];
    let mut hasher = tiny_keccak::Keccak::v256();
    hasher.update(&input[..]);
    hasher.finalize(&mut output);
    output
}

pub(crate) fn public_key_bytes_to_eth_address(public_key: &[u8]) -> Vec<u8> {
    let public_hash = keccak(public_key); // 32 bytes
    let public_hash_tail: &[u8] = &public_hash[12..]; // bytes 12 to 32 - last 20 bytes
    Vec::from(public_hash_tail)
}

pub(crate) fn public_key_to_bytes(public: PublicKey) -> Vec<u8> {
    // some tag is added here - i think for SCALE encoding but [1..] strips it
    let serialized: &[u8] = &public.serialize()[1..];
    let serialized: Vec<u8> = serialized.iter().map(Clone::clone).collect();
    serialized
}

pub(crate) fn public_key_to_eth_address(public: PublicKey) -> Vec<u8> {
    let bytes = public_key_to_bytes(public);
    let address = public_key_bytes_to_eth_address(&bytes);

    address
}

/// Recovers the signer's address from the given signature and message. The message is _not_
/// expected to be a digest and is hashed inside.
pub fn eth_recover(message: &[u8], sig: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let last_byte_of_signature = sig[sig.len() - 1];
    let recovery_id = secp256k1::RecoveryId::parse_rpc(last_byte_of_signature)
        .map_err(|_| CryptoError::ParseError)?;
    let sig = secp256k1::Signature::parse_slice(&sig[..64]).map_err(|_| CryptoError::ParseError)?;
    let digested = eth_keccak_for_signature(&message);
    let message =
        secp256k1::Message::parse_slice(&digested).map_err(|_| CryptoError::ParseError)?;

    let recovered =
        secp256k1::recover(&message, &sig, &recovery_id).map_err(|_| CryptoError::RecoverError)?;
    let address = public_key_to_eth_address(recovered);

    Ok(address)
}
