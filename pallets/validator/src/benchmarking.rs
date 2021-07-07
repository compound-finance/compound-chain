#![cfg(feature = "runtime-benchmarks")]

use super::*;
use crate::{
    chains::{Chain, ChainAsset, ChainSignatureList, Ethereum},
    notices::{ExtractionNotice, Notice},
    rates::APR,
    types::*,
    types::{AssetInfo, Factor, ValidatorKeys},
    Pallet as Cash,
};
use codec::EncodeLike;
use frame_benchmarking::{benchmarks, impl_benchmark_test_suite};
pub use frame_support::{
    assert_err, assert_ok,
    traits::{OnInitialize, OriginTrait},
    StorageValue,
};
use frame_system::RawOrigin;
use hex_literal::hex;
use num_traits::Zero;
use sp_core::crypto::AccountId32;
use sp_std::prelude::*;

pub use our_std::{convert::TryInto, str::FromStr};
use pallet_oracle::Prices;

const TKN_ADDR: &str = "0x0101010101010101010101010101010101010101";
const TKN_ADDR_BYTES: [u8; 20] = [1; 20];

const ETH_ADDR: &str = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE";
const ETH_BYTES: [u8; 20] = hex!("EeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE");
const ETH_UNIT: Units = Units::from_ticker_str("ETH", 18);

const ALICE_ADDRESS: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
const BOB_ADDRESS: &str = "0x59a055a3e566F5d9A9Ea1dA81aB375D5361D7c5e";
const BOB_ADDRESS_BYTES: [u8; 20] = hex!("59a055a3e566F5d9A9Ea1dA81aB375D5361D7c5e");

benchmarks! {
    where_clause {
    where
        T: pallet_session::Config,
    T: pallet_timestamp::Config,
    u64: EncodeLike<<T as pallet_timestamp::Config>::Moment>,
    <<T as frame_system::Config>::Origin as OriginTrait>::AccountId: From<SubstrateId>
}

    // todo: parameterize over # vals?
    change_validators {
        let substrate_id: SubstrateId = [2; 32].into();
        let eth_address = [1; 20];
        let val_keys = ValidatorKeys {
            substrate_id: substrate_id.clone(),
            eth_address: eth_address.clone(),
        };
        let val_keyses = vec![val_keys];
        let val_account = ChainAccount::Gate(substrate_id.clone().into());

        // Min balance needed for account existence, to set session keys
        let min_amount = params::MIN_PRINCIPAL_GATE.amount_withdrawable().unwrap();
        ChainCashPrincipals::insert(ChainId::Gate, min_amount);
        assert_ok!(internal::lock::lock_cash_principal_internal::<T>(
            val_account,
            val_account,
            min_amount
        ));

        // Set session key
        assert_eq!(
            pallet_session::Module::<T>::set_keys(
                T::Origin::signed(substrate_id.into()),
                <T>::Keys::default(),
                vec![]
            ),
            Ok(())
        );
    }: {
        assert_eq!(Cash::<T>::change_validators(RawOrigin::Root.into(), val_keyses), Ok(()));
    }
}

impl_benchmark_test_suite!(Cash, crate::tests::new_test_ext(), crate::tests::Test,);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{
        initialize_storage,
        mock::{new_test_ext, Test},
    };

    #[test]
    fn test_benchmarks() {
        new_test_ext().execute_with(|| {
            initialize_storage();
            assert_ok!(test_benchmark_change_validators::<Test>());
        });
    }
}
