#[macro_use]
extern crate alloc;

use crate::{error::ValidatorError, types::*};
use frame_support::{
    decl_event, decl_module, decl_storage, dispatch,
    traits::{StoredMap, UnfilteredDispatchable},
    weights::{DispatchClass, GetDispatchInfo, Pays, Weight},
    Parameter,
};
use frame_system;
use frame_system::{ensure_none, ensure_root, offchain::CreateSignedTransaction};
use our_std::{
    collections::btree_map::BTreeMap, collections::btree_set::BTreeSet, convert::TryInto, debug,
    error, log, str, vec::Vec, warn, Debuggable,
};
use sp_runtime::{
    transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidity},
    Percent,
};

pub mod error;
pub mod serdes;
pub mod types;
pub mod validate_trx;
pub mod validator;
pub mod weights;

#[cfg(test)]
mod tests;

/// Configure the pallet by specifying the parameters and types on which it depends.
pub trait Config:
    frame_system::Config + CreateSignedTransaction<Call<Self> + pallet_session::Config>
{
    /// Because this pallet emits events, it depends on the runtime's definition of an event.
    type Event: From<Event> + Into<<Self as frame_system::Config>::Event>;

    /// The overarching dispatch call type.
    type Call: From<Call<Self>>
        + Parameter
        + UnfilteredDispatchable<Origin = Self::Origin>
        + GetDispatchInfo;

    /// Associated type which allows us to interact with substrate Sessions.
    type SessionInterface: self::SessionInterface<SubstrateId>;
}

decl_storage! {
    trait Store for Pallet<T: Config> as Cash {
        /// The upcoming session at which to tell the sessions pallet to rotate the validators.
        NextSessionIndex get(fn next_session_index): SessionIndex;

        /// The upcoming set of allowed validators, and their associated keys (or none).
        NextValidators get(fn next_validators): map hasher(blake2_128_concat) SubstrateId => Option<ValidatorKeys>;

        /// The current set of allowed validators, and their associated keys.
        Validators get(fn validators): map hasher(blake2_128_concat) SubstrateId => Option<ValidatorKeys>;
    }
    add_extra_genesis {
        config(validators): Vec<ValidatorKeys>;
        build(|config| {
            Pallet::<T>::initialize_validators(config.validators.clone());
        })
    }
}

/* ::EVENTS:: */

decl_event!(
    pub enum Event {
        /// A new validator set has been chosen. [validators]
        ChangeValidators(Vec<ValidatorKeys>),

        /// Failed to process a given extrinsic. [reason]
        Failure(ValidatorError),
    }
);

/* ::ERRORS:: */

fn check_failure<T: Config>(res: Result<(), ValidatorError>) -> Result<(), ValidatorError> {
    if let Err(err) = res {
        <Pallet<T>>::deposit_event(Event::Failure(err));
        log!("Validator Failure {:#?}", err);
    }
    res
}

/* ::MODULE:: */
/* ::EXTRINSICS:: */

// Dispatchable functions allows users to interact with the pallet and invoke state changes.
// These functions materialize as "extrinsics", which are often compared to transactions.
// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
decl_module! {
    pub struct Pallet<T: Config> for enum Call where origin: T::Origin {
        // Events must be initialized if they are used by the pallet.
        fn deposit_event() = default;

        /// Sets the keys for the next set of validators beginning at the next session. [Root]
        #[weight = (<T as Config>::WeightInfo::change_validators(), DispatchClass::Operational, Pays::No)]
        pub fn change_validators(origin, validators: Vec<ValidatorKeys>) -> dispatch::DispatchResult {
            ensure_root(origin)?;
            Ok(check_failure::<T>(validator::change_validators::<T>(validators))?)
        }
    }
}

/// Reading error messages inside `decl_module!` can be difficult, so we move them here.
impl<T: Config> Pallet<T> {
    /// Set the initial set of validators from the genesis config.
    /// NextValidators will become current Validators upon first session start.
    fn initialize_validators(validators: Vec<ValidatorKeys>) {
        if validators.is_empty() {
            warn!("Validators must be set in the genesis config");
        }
        for validator in validators {
            // Note: See pipeline commit for usage of T::AccountStore
            log!("Adding validator: {:?}", validator);
            <Validators>::insert(&validator.substrate_id, validator.clone());
            assert!(T::AccountStore::insert(&validator.substrate_id, ()).is_ok());
        }
    }

    // ** API / View Functions ** //
}

impl<T: Config> frame_support::unsigned::ValidateUnsigned for Pallet<T> {
    type Call = Call<T>;

    /// Validate unsigned call to this module.
    ///
    /// By default unsigned transactions are disallowed, but implementing the validator
    /// here we make sure that some particular calls (the ones produced by offchain worker)
    /// are being whitelisted and marked as valid.
    fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
        // TODO: Why like this?
        validate_trx::check_validation_failure(
            call,
            validate_trx::validate_unsigned::<T>(source, call),
        )
        .unwrap_or(InvalidTransaction::Call.into())
    }
}

pub trait SessionInterface<AccountId>: frame_system::Config {
    fn has_next_keys(x: AccountId) -> bool;
    fn rotate_session();
}

impl<T: Config> SessionInterface<SubstrateId> for T
where
    T: pallet_session::Config<ValidatorId = SubstrateId>,
{
    fn has_next_keys(x: SubstrateId) -> bool {
        match <pallet_session::Pallet<T>>::next_keys(x as T::ValidatorId) {
            Some(_keys) => true,
            None => false,
        }
    }

    fn rotate_session() {
        <pallet_session::Pallet<T>>::rotate_session();
    }
}

impl<T: Config> pallet_session::SessionManager<SubstrateId> for Pallet<T> {
    // return validator set to use in the next session (aura and grandpa also stage new auths associated w these accountIds)
    fn new_session(session_index: SessionIndex) -> Option<Vec<SubstrateId>> {
        if NextValidators::iter().count() != 0 {
            NextSessionIndex::put(session_index);
            Some(NextValidators::iter().map(|x| x.0).collect::<Vec<_>>())
        } else {
            Some(Validators::iter().map(|x| x.0).collect::<Vec<_>>())
        }
    }

    fn start_session(index: SessionIndex) {
        // if changes have been queued
        // if starting the queued session
        if NextSessionIndex::get() == index && NextValidators::iter().count() != 0 {
            // delete existing validators
            for validator in <Validators>::iter_values() {
                <Validators>::take(&validator.substrate_id);
            }
            // push next validators into current validators
            for (id, validator) in <NextValidators>::iter() {
                <NextValidators>::take(&id);
                <Validators>::insert(&id, validator);
            }
        } else {
            ()
        }
    }
    fn end_session(_: SessionIndex) {
        ()
    }
}

/*
-- Block N --
changeAuth extrinsic, nextValidators set, hold is set, rotate_session is called
* new_session returns the nextValidators

-- Afterwards --
"ShouldEndSession" returns true when notice era notices were signed
* when it does, start_session sets Validators = NextValidators

*/

fn vec_to_set<T: Ord + Debuggable>(a: Vec<T>) -> BTreeSet<T> {
    let mut a_set = BTreeSet::<T>::new();
    for v in a {
        a_set.insert(v);
    }
    return a_set;
}

// fn has_requisite_signatures(notice_state: NoticeState, validators: &Vec<ValidatorKeys>) -> bool {
//     match notice_state {
//         NoticeState::Pending { signature_pairs } => match signature_pairs {
//             // TODO: This is currently dependent on Notices and ChainSignatureList?
//             ChainSignatureList::Eth(signature_pairs) => {
//                 // Note: inefficient, probably best to store as sorted lists / zip compare
//                 type EthAddrType = <chains::Ethereum as chains::Chain>::Address;
//                 let signature_set =
//                     vec_to_set::<EthAddrType>(signature_pairs.iter().map(|p| p.0).collect());
//                 let validator_set =
//                     vec_to_set::<EthAddrType>(validators.iter().map(|v| v.eth_address).collect());
//                 chains::has_super_majority::<EthAddrType>(&signature_set, &validator_set)
//             }
//             _ => false,
//         },
//         _ => false,
//     }
// }

// periodic except when new authorities are pending and when an era notice has just been completed
impl<T: Config> pallet_session::ShouldEndSession<T::BlockNumber> for Pallet<T> {
    fn should_end_session(now: T::BlockNumber) -> bool {
        if NextValidators::iter().count() > 0 {
            // Check if we should end the hold
            let validators: Vec<_> = Validators::iter().map(|v| v.1).collect();

            // TODO: Add back notice hold checks
            // // TODO: How to deal with notice holds, etc?
            // let every_notice_hold_executed = NoticeHolds::iter().all(|(chain_id, notice_id)| {
            //     has_requisite_signatures(NoticeStates::get(chain_id, notice_id), &validators)
            // });

            // if every_notice_hold_executed {
            //     for (chain_id, _) in NoticeHolds::iter() {
            //         NoticeHolds::take(chain_id);
            //     }
            //     log!("should_end_session=true[next_validators]");
            //     true
            // } else {
            //     log!("should_end_session=false[pending_notice_held]");
            //     false
            // }
            true
        } else {
            // no era changes pending, periodic
            let period: T::BlockNumber = <T>::BlockNumber::from(params::SESSION_PERIOD as u32);
            let is_new_period = (now % period) == <T>::BlockNumber::from(0 as u32);

            if is_new_period {
                log!(
                    "should_end_session={}[periodic {:?}%{:?}]",
                    is_new_period,
                    now,
                    period
                );
            }
            is_new_period
        }
    }
}

impl<T: Config> frame_support::traits::EstimateNextSessionRotation<T::BlockNumber> for Pallet<T> {
    fn average_session_length() -> T::BlockNumber {
        T::BlockNumber::zero()
    }

    fn estimate_current_session_progress(now: T::BlockNumber) -> (Option<Percent>, Weight) {
        let period: T::BlockNumber = <T>::BlockNumber::from(params::SESSION_PERIOD as u32);
        (
            Some(Percent::from_rational(now % period, period)),
            Weight::zero(),
        )
    }

    fn estimate_next_session_rotation(now: T::BlockNumber) -> (Option<T::BlockNumber>, Weight) {
        let period: T::BlockNumber = <T>::BlockNumber::from(params::SESSION_PERIOD as u32);
        (Some(now + period - now % period), Weight::zero())
    }
}
