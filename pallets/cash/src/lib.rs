#![cfg_attr(not(feature = "std"), no_std)]

use crate::amount::{Amount, CashAmount};
use crate::notices::{Notice, EthHash};
use crate::account::{AccountAddr, AccountIdent, ChainIdent};
use codec::alloc::string::String;
/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// https://substrate.dev/docs/en/knowledgebase/runtime/frame
use frame_support::{
    debug, decl_error, decl_event, decl_module, decl_storage, dispatch, traits::Get,
};

use frame_system::{ensure_none, ensure_signed,
    offchain::{ AppCrypto, SubmitTransaction, CreateSignedTransaction }
};
use sp_runtime::{
    offchain::http,
    transaction_validity::{
       TransactionSource, TransactionValidity, ValidTransaction,
    },
};
use sp_std::vec::Vec;
use sp_std::prelude::Box;
use sp_core::crypto::{KeyTypeId};

#[cfg(test)]
mod mock;

mod account;
mod amount;

#[cfg(test)]
mod tests;

#[macro_use]
extern crate alloc;

mod notices;

extern crate ethereum_client;

pub const ETH_KEY_TYPE: KeyTypeId = KeyTypeId(*b"eth!");

pub mod crypto {
    use crate::ETH_KEY_TYPE;
    use sp_core::ecdsa::Signature as EcdsaSignature;

    use sp_runtime::app_crypto::{app_crypto, ecdsa};
    use sp_runtime::{traits::Verify, MultiSignature, MultiSigner };

    app_crypto!(ecdsa, ETH_KEY_TYPE);

    pub struct TestAuthId;
	// implemented for ocw-runtime
	impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for TestAuthId {
		type RuntimeAppPublic = Public;
        type GenericSignature = sp_core::ecdsa::Signature;
        type GenericPublic = sp_core::ecdsa::Public;
    }
    
    	// implemented for mock runtime in test
	impl frame_system::offchain::AppCrypto<<EcdsaSignature as Verify>::Signer, EcdsaSignature>
    for TestAuthId
{
    type RuntimeAppPublic = Public;
    type GenericSignature = sp_core::ecdsa::Signature;
    type GenericPublic = sp_core::ecdsa::Public;
}
}

/// Configure the pallet by specifying the parameters and types on which it depends.
pub trait Config: frame_system::Config + CreateSignedTransaction<Call<Self>> {
    /// Because this pallet emits events, it depends on the runtime's definition of an event.
    type Event: From<Event<Self>> + Into<<Self as frame_system::Config>::Event>;

    type AuthorityId: AppCrypto<Self::Public, Self::Signature>;

    /// The overarching dispatch call type.
    type Call: From<Call<Self>>;
}

decl_storage! {
    trait Store for Module<T: Config> as Cash {
        // XXX
        // Learn more about declaring storage items:
        // https://substrate.dev/docs/en/knowledgebase/runtime/storage#declaring-storage-items
        Something get(fn something): Option<u32>;

        // XXX
        CashBalance get(fn cash_balance): map hasher(blake2_128_concat) AccountIdent => Option<CashAmount>;
        // TODO: hash type should match to ChainIdent
        pub NoticeQueue get(fn notice_queue): double_map hasher(blake2_128_concat) ChainIdent, hasher(blake2_128_concat) EthHash => Option<Notice>;

    }
}

decl_event!(
    pub enum Event<T>
    where
        AccountId = <T as frame_system::Config>::AccountId,
    {
        // XXX
        /// Event documentation should end with an array that provides descriptive names for event
        /// parameters. [something, who]
        SomethingStored(u32, AccountId),

        // XXX
        MagicExtract(CashAmount, AccountIdent, Notice),
        Notice(notices::Message, notices::Signature, AccountIdent),
    }
);

decl_error! {
    pub enum Error for Module<T: Config> {
        // XXX
        /// Error names should be descriptive.
        NoneValue,
        /// Errors should have helpful documentation associated with them.
        StorageOverflow,
    }
}

// Dispatchable functions allows users to interact with the pallet and invoke state changes.
// These functions materialize as "extrinsics", which are often compared to transactions.
// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
decl_module! {
    pub struct Module<T: Config> for enum Call where origin: T::Origin {
        // Errors must be initialized if they are used by the pallet.
        type Error = Error<T>;

        // Events must be initialized if they are used by the pallet.
        fn deposit_event() = default;

        /// An example dispatchable that takes a singles value as a parameter, writes the value to
        /// storage and emits an event. This function must be dispatched by a signed extrinsic.
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn magic_extract(origin, account: AccountIdent, amount: CashAmount) -> dispatch::DispatchResult {
            let () = ensure_none(origin)?;

            // Update storage -- TODO: increment this-- sure why not?
            let curr_cash_balance: CashAmount = CashBalance::get(&account).unwrap_or_default();
            let next_cash_balance: CashAmount = curr_cash_balance.checked_add(amount).ok_or(Error::<T>::StorageOverflow)?;
            CashBalance::insert(&account, next_cash_balance);

            // Add to Notice Queue
            let notice = Notice::ExtractionNotice {asset: Vec::new(), amount: Amount::new_cash(amount), account: account.clone()};
            let dummy_hash: [u8; 32] = [0; 32];
            NoticeQueue::insert(ChainIdent::Eth, dummy_hash, &notice);

            // Emit an event.
            Self::deposit_event(RawEvent::MagicExtract(amount, account, notice));
            // Return a successful DispatchResult
            Ok(())
        }

        /// An example dispatchable that takes a singles value as a parameter, writes the value to
        /// storage and emits an event. This function must be dispatched by a signed extrinsic.
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn do_something(origin, something: u32) -> dispatch::DispatchResult {
            let who = ensure_signed(origin)?;

            // Update storage.
            Something::put(something);

            // Emit an event.
            Self::deposit_event(RawEvent::SomethingStored(something, who));
            // Return a successful DispatchResult
            Ok(())
        }

        /// An example dispatchable that may throw a custom error.
        #[weight = 10_000 + T::DbWeight::get().reads_writes(1,1)]
        pub fn cause_error(origin) -> dispatch::DispatchResult {
            let _who = ensure_signed(origin)?;
            let _ = runtime_interfaces::config_interface::get();

            // Read a value from storage.
            match Something::get() {
                // Return an error if the value has not been set.
                None => Err(Error::<T>::NoneValue)?,
                Some(old) => {
                    // Increment the value read from storage; will error in the event of overflow.
                    let new = old.checked_add(1).ok_or(Error::<T>::StorageOverflow)?;
                    // Update the value in storage with the incremented result.
                    Something::put(new);
                    Ok(())
                },
            }
        }

        #[weight = 0]
        pub fn emit_notice(origin, notice: notices::NoticePayload) -> dispatch::DispatchResult {
            // TODO: Move to using unsigned and getting author from signature
            // TODO I don't know what this comment means ^
            ensure_none(origin)?;
    
            debug::native::info!("emitting notice: {:?}", notice);
            Self::deposit_event(RawEvent::Notice(notice.msg, notice.sig, notice.signer));
    
            Ok(())
        }

        /// Offchain Worker entry point.
        ///
        /// By implementing `fn offchain_worker` within `decl_module!` you declare a new offchain
        /// worker.
        /// This function will be called when the node is fully synced and a new best block is
        /// succesfuly imported.
        /// Note that it's not guaranteed for offchain workers to run on EVERY block, there might
        /// be cases where some blocks are skipped, or for some the worker runs twice (re-orgs),
        /// so the code should be able to handle that.
        /// You can use `Local Storage` API to coordinate runs of the worker.
        fn offchain_worker(block_number: T::BlockNumber) {
            debug::native::info!("Hello World from offchain workers!");
            let config = runtime_interfaces::config_interface::get();
            let eth_rpc_url = String::from_utf8(config.get_eth_rpc_url()).unwrap();
            // debug::native::info!("CONFIG: {:?}", eth_rpc_url);

            // TODO create parameter vector from storage variables
            let lock_events: Result<Vec<ethereum_client::LogEvent<ethereum_client::LockEvent>>, http::Error> = ethereum_client::fetch_and_decode_events(&eth_rpc_url, vec!["{\"address\": \"0x3f861853B41e19D5BBe03363Bb2f50D191a723A2\", \"fromBlock\": \"0x146A47D\", \"toBlock\" : \"latest\", \"topics\":[\"0xddd0ae9ae645d3e7702ed6a55b29d04590c55af248d51c92c674638f3fb9d575\"]}"]);
            debug::native::info!("Lock Events: {:?}", lock_events);
            Self::process_notices(block_number);
        }
    }
}

impl<T: Config> Module<T> {
    pub fn process_notices(block_number: T::BlockNumber) {
        let n = notices::Notice::ExtractionNotice{
            asset: "eth:0xfffff".as_bytes().to_vec(),
            account: AccountIdent{chain: ChainIdent::Eth, account: "eth:0xF33d".as_bytes().to_vec() },
            amount: Amount::new(2000_u32, 3)
        };

        // let pending_notices : Vec<Box<Notice>> =  vec![Box::new(n)];
        let pending_notices : Vec<Notice> =  vec![n];

        for notice in pending_notices.iter() {
            // find parent
            // id = notice.gen_id(parent)
        
            // submit onchain call for aggregating the price
            let payload = notices::to_payload(notice);
            let call = Call::emit_notice(payload);
        
            // Unsigned tx
            SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(call.into());
        };
    }
}

impl<T: Config> frame_support::unsigned::ValidateUnsigned for Module<T> {
    type Call = Call<T>;

    /// Validate unsigned call to this module.
    ///
    /// By default unsigned transactions are disallowed, but implementing the validator
    /// here we make sure that some particular calls (the ones produced by offchain worker)
    /// are being whitelisted and marked as valid.
    fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
        // TODO: This is not ready for prime-time
        ValidTransaction::with_tag_prefix("CashPallet")
            // The transaction is only valid for next 10 blocks. After that it's
            // going to be revalidated by the pool.
            .longevity(10)
            .and_provides("fix_this_function")
            // It's fine to propagate that transaction to other peers, which means it can be
            // created even by nodes that don't produce blocks.
            // Note that sometimes it's better to keep it for yourself (if you are the block
            // producer), since for instance in some schemes others may copy your solution and
            // claim a reward.
            .propagate(true)
            .build()
    }
}
