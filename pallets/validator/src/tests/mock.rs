use crate::{self as pallet_oracle, *};
use codec::alloc::sync::Arc;
use parking_lot::RwLock;
use sp_core::{
    offchain::{
        testing::{self, OffchainState, PoolState},
        OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
    },
    H256,
};
use sp_runtime::{
    generic,
    testing::{Header, TestXt},
    traits::{BlakeTwo256, Extrinsic as ExtrinsicT, IdentifyAccount, IdentityLookup, Verify},
    MultiAddress, MultiSignature as Signature,
};

pub type Extrinsic = TestXt<Call, ()>;
pub type OracleModule = Module<Test>;
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

pub type Address = MultiAddress<AccountId, ()>;
pub type SignedExtra = (
    frame_system::CheckSpecVersion<Test>,
    frame_system::CheckTxVersion<Test>,
    frame_system::CheckGenesis<Test>,
    frame_system::CheckEra<Test>,
    frame_system::CheckNonce<Test>,
    frame_system::CheckWeight<Test>,
);
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, Call, Signature, SignedExtra>;
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

pub mod opaque {
    pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;
}

pub const MILLISECS_PER_BLOCK: types::Timestamp = 6000;
pub const SLOT_DURATION: types::Timestamp = MILLISECS_PER_BLOCK;

frame_support::parameter_types! {
    pub const MinimumPeriod: types::Timestamp = SLOT_DURATION / 2;
}

frame_support::construct_runtime!(
    pub enum Test where
    Block = Block,
    NodeBlock = Block,
    UncheckedExtrinsic = UncheckedExtrinsic,
    {
        System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
        Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
        Oracle: pallet_oracle::{Pallet, Call, Config, Storage, Event, Inherent},
    }
);

frame_support::parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
}

impl frame_system::Config for Test {
    type BaseCallFilter = ();
    type BlockWeights = ();
    type BlockLength = ();
    type DbWeight = ();
    type Origin = Origin;
    type Call = Call;
    type Index = u64;
    type BlockNumber = u64;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Header = Header;
    type Event = Event;
    type BlockHashCount = BlockHashCount;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = ();
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type OnSetCode = ();
    type SystemWeightInfo = ();
    type SS58Prefix = SS58Prefix;
}

impl frame_system::offchain::SigningTypes for Test {
    type Public = <Signature as Verify>::Signer;
    type Signature = Signature;
}

impl Config for Test {
    type Event = Event;
    type Call = Call;
    type GetConvertedTimestamp = timestamp::TimeConverter<Self>;
}
impl pallet_timestamp::Config for Test {
    /// A timestamp: milliseconds since the unix epoch.
    type Moment = types::Timestamp;
    type OnTimestampSet = ();
    type MinimumPeriod = MinimumPeriod;
    type WeightInfo = ();
}

impl<LocalCall> frame_system::offchain::SendTransactionTypes<LocalCall> for Test
where
    Call: From<LocalCall>,
{
    type OverarchingCall = Call;
    type Extrinsic = Extrinsic;
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Test
where
    Call: From<LocalCall>,
{
    fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
        call: Call,
        _public: <Signature as Verify>::Signer,
        _account: AccountId,
        nonce: u64,
    ) -> Option<(Call, <Extrinsic as ExtrinsicT>::SignaturePayload)> {
        Some((call, (nonce, ())))
    }
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
    let (t, _pool_state, _offchain_state) = new_test_ext_with_http_calls(vec![]);
    t
}

pub fn new_test_ext_with_http_calls(
    mut calls: Vec<testing::PendingRequest>,
) -> (
    sp_io::TestExternalities,
    Arc<RwLock<PoolState>>,
    Arc<RwLock<OffchainState>>,
) {
    let (offchain, offchain_state) = testing::TestOffchainExt::new();
    let (pool, pool_state) = testing::TestTransactionPoolExt::new();

    // XXX
    // let mut test_externalities: sp_io::TestExternalities = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap().into();
    let mut test_externalities = sp_io::TestExternalities::default();
    test_externalities.register_extension(OffchainDbExt::new(offchain.clone()));
    test_externalities.register_extension(OffchainWorkerExt::new(offchain));
    test_externalities.register_extension(TransactionPoolExt::new(pool));

    {
        let mut state = offchain_state.write();
        for call in calls.drain(0..calls.len()) {
            state.expect_request(call);
        }
    }

    test_externalities.execute_with(|| System::set_block_number(1));
    (test_externalities, pool_state, offchain_state)
}
