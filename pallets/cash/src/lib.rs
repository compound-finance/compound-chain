#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// https://substrate.dev/docs/en/knowledgebase/runtime/frame
use frame_support::{
    debug, decl_error, decl_event, decl_module, decl_storage, dispatch, traits::Get,
};
use frame_system::ensure_signed;
use sp_runtime::offchain::http;
use sp_std::vec::Vec;

extern crate ethereum_client;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[macro_use]
extern crate alloc;

/// Configure the pallet by specifying the parameters and types on which it depends.
pub trait Trait: frame_system::Trait {
    /// Because this pallet emits events, it depends on the runtime's definition of an event.
    type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;
}

// The pallet's runtime storage items.
// https://substrate.dev/docs/en/knowledgebase/runtime/storage
decl_storage! {
    // A unique name is used to ensure that the pallet's storage items are isolated.
    // This name may be updated, but each pallet in the runtime must use a unique name.
    // ---------------------------------vvvvvvvvvvvvvv
    trait Store for Module<T: Trait> as TemplateModule {
        // Learn more about declaring storage items:
        // https://substrate.dev/docs/en/knowledgebase/runtime/storage#declaring-storage-items
        Something get(fn something): Option<u32>;
    }
}

// Pallets use events to inform users when important changes are made.
// https://substrate.dev/docs/en/knowledgebase/runtime/events
decl_event!(
    pub enum Event<T>
    where
        AccountId = <T as frame_system::Trait>::AccountId,
    {
        /// Event documentation should end with an array that provides descriptive names for event
        /// parameters. [something, who]
        SomethingStored(u32, AccountId),
    }
);

// Errors inform users that something went wrong.
decl_error! {
    pub enum Error for Module<T: Trait> {
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
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        // Errors must be initialized if they are used by the pallet.
        type Error = Error<T>;

        // Events must be initialized if they are used by the pallet.
        fn deposit_event() = default;

        /// An example dispatchable that takes a singles value as a parameter, writes the value to
        /// storage and emits an event. This function must be dispatched by a signed extrinsic.
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn do_something(origin, something: u32) -> dispatch::DispatchResult {
            // Check that the extrinsic was signed and get the signer.
            // This function will return an error if the extrinsic is not signed.
            // https://substrate.dev/docs/en/knowledgebase/runtime/origin
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

            let lock_events: Result<Vec<ethereum_client::LogEvent<ethereum_client::LockEvent>>, http::Error> = ethereum_client::fetch_and_decode_events("https://kovan.infura.io/v3/975c0c48e2ca4649b7b332f310050e27",vec!["{\"address\": \"0x3f861853B41e19D5BBe03363Bb2f50D191a723A2\", \"fromBlock\": \"0x146A47D\", \"toBlock\" : \"latest\", \"topics\":[\"0xddd0ae9ae645d3e7702ed6a55b29d04590c55af248d51c92c674638f3fb9d575\"]}"]);
            debug::native::info!("Lock Events: {:?}", lock_events);
        }
    }
}
