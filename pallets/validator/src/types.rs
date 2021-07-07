use sp_core::crypto::AccountId32;

use our_std::collections::btree_set::BTreeSet;
use types_derive::{type_alias, Types};

/// Type for enumerating sessions.
#[type_alias]
pub type SessionIndex = u32;

/// Type for an address used to identify a validator.
#[type_alias]
pub type ValidatorIdentity = SubstrateId;

/// Type for signers set used to identify validators that signed this event.
#[type_alias]
pub type SignersSet = BTreeSet<ValidatorIdentity>;

/// Type for representing the keys to sign notices.
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug, Types)]
pub struct ValidatorKeys {
    pub substrate_id: SubstrateId,
    pub eth_address: [u32; 20], // TODO: <Ethereum as Chain>::Address
}

/// Type for linking sessions to validators.
#[type_alias]
pub type SubstrateId = AccountId32;
