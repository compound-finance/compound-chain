// Note: The substrate build requires these be re-exported.
pub use our_std::{
    cmp::{max, min},
    collections::btree_set::BTreeSet,
    convert::{TryFrom, TryInto},
    fmt, result,
    result::Result,
    str,
};

use codec::Decode;
use frame_support::traits::UnfilteredDispatchable;
use frame_support::{
    sp_runtime::traits::Convert,
    storage::{
        IterableStorageDoubleMap, IterableStorageMap, StorageDoubleMap, StorageMap, StorageValue,
    },
};

// Import these traits so we can interact with the substrate storage modules.
use crate::{
    chains::{ChainAccount, ChainAsset, ChainAssetAccount, ChainHash, ChainId},
    events::ChainLogEvent,
    factor::Factor,
    internal, log,
    notices::{
        CashExtractionNotice, EncodeNotice, ExtractionNotice, Notice, NoticeId, NoticeState,
    },
    params::{MIN_TX_VALUE, TRANSFER_FEE},
    portfolio::Portfolio,
    rates::APR,
    reason::{MathError, Reason},
    symbol::{Ticker, Units, CASH, USD},
    types::{
        AssetAmount, AssetBalance, AssetIndex, AssetInfo, AssetQuantity, Balance, CashIndex,
        CashPrincipal, CashPrincipalAmount, CashQuantity, Price, Quantity, Timestamp, USDQuantity,
        ValidatorIdentity,
    },
    AccountNotices, AssetBalances, AssetsWithNonZeroBalance, BorrowIndices, CashPrincipals,
    CashYield, CashYieldNext, ChainCashPrincipals, Config, Event, GlobalCashIndex, LastIndices,
    LastMinerSharePrincipal, LastYieldTimestamp, LatestNotice, Miner, Module, NoticeHashes,
    NoticeStates, Notices, Prices, SupplyIndices, SupportedAssets, TotalBorrowAssets,
    TotalCashPrincipal, TotalSupplyAssets,
};

#[macro_export]
macro_rules! require {
    ($expr:expr, $reason:expr) => {
        if !$expr {
            return core::result::Result::Err($reason);
        }
    };
}

#[macro_export]
macro_rules! require_min_tx_value {
    ($value:expr) => {
        require!($value >= MIN_TX_VALUE, Reason::MinTxValueNotMet);
    };
}

// Public helper functions //

// Miner might not be set (e.g. in the first block mined), but for accouting
// purposes, we want some address to make sure all numbers tie out. As such,
// let's just give the initial rewards to some burn account.
pub fn get_some_miner<T: Config>() -> ChainAccount {
    Miner::get().unwrap_or(ChainAccount::Eth([0; 20]))
}

pub fn try_chain_asset_account(
    asset: ChainAsset,
    account: ChainAccount,
) -> Option<ChainAssetAccount> {
    match (asset, account) {
        (ChainAsset::Eth(eth_asset), ChainAccount::Eth(eth_account)) => {
            Some(ChainAssetAccount::Eth(eth_asset, eth_account))
        }
        _ => None,
    }
}

pub fn get_now<T: Config>() -> Timestamp {
    let now = <pallet_timestamp::Module<T>>::get();
    T::TimeConverter::convert(now)
}

/// Return the full asset info for an asset.
pub fn get_asset<T: Config>(asset: ChainAsset) -> Result<AssetInfo, Reason> {
    Ok(SupportedAssets::get(asset).ok_or(Reason::AssetNotSupported)?)
}

/// Return the USD price associated with the given units.
pub fn get_price_by_ticker<T: Config>(ticker: Ticker) -> Result<Price, Reason> {
    match ticker {
        t if t == USD.ticker => Ok(Price::from_nominal(USD.ticker, "1.0")),
        t if t == CASH.ticker => Ok(Price::from_nominal(CASH.ticker, "1.0")),
        _ => Ok(Price::new(
            ticker,
            // XXX Prices::get(units.ticker).ok_or(Reason::NoPrice)?,
            // XXX fix me how to handle no prices during initialization?
            Prices::get(ticker).unwrap_or(0),
        )),
    }
}

/// Return the USD price associated with the given units.
pub fn get_price<T: Config>(units: Units) -> Result<Price, Reason> {
    get_price_by_ticker::<T>(units.ticker)
}

/// Return a quantity with units of the given asset.
pub fn get_quantity<T: Config>(asset: ChainAsset, amount: AssetAmount) -> Result<Quantity, Reason> {
    Ok(SupportedAssets::get(asset)
        .ok_or(Reason::AssetNotSupported)?
        .as_quantity(amount))
}

/// Return the USD value of the asset amount.
pub fn get_value<T: Config>(amount: AssetQuantity) -> Result<USDQuantity, Reason> {
    Ok(amount.mul_price(get_price::<T>(amount.units)?)?)
}

/// Return the current utilization for the asset.
pub fn get_utilization<T: Config>(asset: ChainAsset) -> Result<Factor, Reason> {
    let _info = SupportedAssets::get(asset).ok_or(Reason::AssetNotSupported)?;
    let total_supply = TotalSupplyAssets::get(asset);
    let total_borrow = TotalBorrowAssets::get(asset);
    Ok(crate::rates::get_utilization(total_supply, total_borrow)?)
}

/// Return the current borrow and supply rates for the asset.
pub fn get_rates<T: Config>(asset: ChainAsset) -> Result<(APR, APR), Reason> {
    let info = SupportedAssets::get(asset).ok_or(Reason::AssetNotSupported)?;
    let utilization = get_utilization::<T>(asset)?;
    Ok(info
        .rate_model
        .get_rates(utilization, APR::ZERO, info.miner_shares)?)
}

// Internal helpers

// XXX we should receive the sets as args
pub fn passes_validation_threshold(
    signers: &Vec<ValidatorIdentity>,
    validators: &Vec<ValidatorIdentity>,
) -> bool {
    let mut signer_set = BTreeSet::<ValidatorIdentity>::new();
    for v in signers {
        signer_set.insert(*v);
    }

    let mut validator_set = BTreeSet::<ValidatorIdentity>::new();
    for v in validators {
        validator_set.insert(*v);
    }
    signer_set.len() > validator_set.len() * 2 / 3
}

// XXX use Balances instead of raw balances everywhere and put all fns on types?

fn add_amount_to_raw(a: AssetAmount, b: AssetQuantity) -> Result<AssetAmount, MathError> {
    Ok(a.checked_add(b.value).ok_or(MathError::Overflow)?)
}

fn add_amount_to_balance(
    balance: AssetBalance,
    amount: AssetQuantity,
) -> Result<AssetBalance, MathError> {
    let signed = AssetBalance::try_from(amount.value).or(Err(MathError::Overflow))?;
    Ok(balance.checked_add(signed).ok_or(MathError::Overflow)?)
}

fn add_principal_amounts(
    a: CashPrincipalAmount,
    b: CashPrincipalAmount,
) -> Result<CashPrincipalAmount, MathError> {
    Ok(a.add(b)?)
}

fn sub_amount_from_raw(
    a: AssetAmount,
    b: AssetQuantity,
    underflow: Reason,
) -> Result<AssetAmount, Reason> {
    Ok(a.checked_sub(b.value).ok_or(underflow)?)
}

fn sub_amount_from_balance(
    balance: AssetBalance,
    amount: AssetQuantity,
) -> Result<AssetBalance, MathError> {
    let signed = AssetBalance::try_from(amount.value).or(Err(MathError::Overflow))?;
    Ok(balance.checked_sub(signed).ok_or(MathError::Underflow)?)
}

fn sub_principal_amounts(
    a: CashPrincipalAmount,
    b: CashPrincipalAmount,
    underflow: Reason,
) -> Result<CashPrincipalAmount, Reason> {
    Ok(a.sub(b).map_err(|_| underflow)?)
}

fn neg_balance(balance: AssetBalance) -> AssetAmount {
    if balance < 0 {
        -balance as AssetAmount
    } else {
        0
    }
}

fn pos_balance(balance: AssetBalance) -> AssetAmount {
    if balance > 0 {
        balance as AssetAmount
    } else {
        0
    }
}

fn repay_and_supply_amount(
    balance: AssetBalance,
    amount: AssetQuantity,
) -> (AssetQuantity, AssetQuantity) {
    let Quantity {
        value: raw_amount,
        units,
    } = amount;
    let repay_amount = min(neg_balance(balance), raw_amount);
    let supply_amount = raw_amount - repay_amount;
    (
        Quantity::new(repay_amount, units),
        Quantity::new(supply_amount, units),
    )
}

fn withdraw_and_borrow_amount(
    balance: AssetBalance,
    amount: AssetQuantity,
) -> (AssetQuantity, AssetQuantity) {
    let Quantity {
        value: raw_amount,
        units,
    } = amount;
    let withdraw_amount = min(pos_balance(balance), raw_amount);
    let borrow_amount = raw_amount - withdraw_amount;
    (
        Quantity::new(withdraw_amount, units),
        Quantity::new(borrow_amount, units),
    )
}

fn repay_and_supply_principal(
    balance: CashPrincipal,
    principal: CashPrincipalAmount,
) -> (CashPrincipalAmount, CashPrincipalAmount) {
    let repay_principal = min(neg_balance(balance.0), principal.0);
    let supply_principal = principal.0 - repay_principal;
    (
        CashPrincipalAmount(repay_principal),
        CashPrincipalAmount(supply_principal),
    )
}

fn withdraw_and_borrow_principal(
    balance: CashPrincipal,
    principal: CashPrincipalAmount,
) -> (CashPrincipalAmount, CashPrincipalAmount) {
    let withdraw_principal = min(pos_balance(balance.0), principal.0);
    let borrow_principal = principal.0 - withdraw_principal;
    (
        CashPrincipalAmount(withdraw_principal),
        CashPrincipalAmount(borrow_principal),
    )
}

// Protocol interface //

pub fn apply_chain_event_internal<T: Config>(event: ChainLogEvent) -> Result<(), Reason> {
    log!("apply_chain_event_internal(event): {:?}", &event);
    match event {
        ChainLogEvent::Eth(eth_event) => match eth_event.event {
            ethereum_client::events::EthereumEvent::Lock {
                asset,
                recipient,
                amount,
                ..
            } => lock_internal::<T>(
                get_asset::<T>(ChainAsset::Eth(asset))?,
                ChainAccount::Eth(recipient),
                get_quantity::<T>(ChainAsset::Eth(asset), amount)?,
            ),

            ethereum_client::events::EthereumEvent::LockCash {
                recipient,
                principal,
                ..
            } => lock_cash_principal_internal::<T>(
                ChainAccount::Eth(recipient),
                CashPrincipalAmount(principal),
            ),

            ethereum_client::events::EthereumEvent::ExecuteProposal {
                title: _title,
                extrinsics,
            } => dispatch_extrinsics_internal::<T>(extrinsics),

            ethereum_client::events::EthereumEvent::ExecTrxRequest {
                account,
                trx_request,
            } => internal::exec_trx_request::exec_trx_request::<T>(
                &trx_request[..],
                ChainAccount::Eth(account),
                None,
            ),

            ethereum_client::events::EthereumEvent::NoticeInvoked {
                era_id,
                era_index,
                notice_hash,
                result,
            } => internal::notices::handle_notice_invoked::<T>(
                ChainId::Eth,
                NoticeId(era_id, era_index),
                ChainHash::Eth(notice_hash),
                result,
            ),
        },
    }
}

pub fn dispatch_notice_internal<T: Config>(
    chain_id: ChainId,
    recipient_opt: Option<ChainAccount>,
    notice_fn: &dyn Fn(NoticeId, ChainHash) -> Result<Notice, Reason>,
) -> Result<(), Reason> {
    // XXX this cannot fail, should not return result
    let (latest_notice_id, parent_hash) =
        LatestNotice::get(chain_id).unwrap_or((NoticeId(0, 0), chain_id.zero_hash()));
    let notice_id = latest_notice_id.seq();

    let notice = notice_fn(notice_id, parent_hash)?; // XXX fixme cannot fail

    // Add to notices, notice states, track the latest notice and index by account
    let notice_hash = notice.hash();
    Notices::insert(chain_id, notice_id, &notice);
    NoticeStates::insert(chain_id, notice_id, NoticeState::pending(&notice));
    LatestNotice::insert(chain_id, (notice_id, notice_hash));
    NoticeHashes::insert(notice_hash, notice_id);
    if let Some(recipient) = recipient_opt {
        AccountNotices::append(recipient, notice_id);
    }

    // Deposit Notice Event
    let encoded_notice = notice.encode_notice();
    Module::<T>::deposit_event(Event::Notice(notice_id, notice, encoded_notice));
    Ok(()) // XXX cannot fail
}

/// Update the index of which assets an account has non-zero balances in.
fn set_asset_balance_internal<T: Config>(
    asset: ChainAsset,
    account: ChainAccount,
    balance: AssetBalance,
) {
    if balance == 0 {
        AssetsWithNonZeroBalance::remove(account, asset);
    } else {
        AssetsWithNonZeroBalance::insert(account, asset, ());
    }

    AssetBalances::insert(asset, account, balance);
}

pub fn dispatch_extrinsics_internal<T: Config>(extrinsics: Vec<Vec<u8>>) -> Result<(), Reason> {
    // Decode a SCALE-encoded set of extrinsics from the event
    // For each extrinsic, dispatch the given extrinsic as Root
    let results: Vec<_> = extrinsics
        .into_iter()
        .map(|payload| {
            log!(
                "dispatch_extrinsics_internal:: dispatching extrinsic {}",
                hex::encode(&payload)
            );
            let call_res: Result<<T as Config>::Call, _> = Decode::decode(&mut &payload[..]);
            match call_res {
                Ok(call) => {
                    log!("dispatch_extrinsics_internal:: dispatching {:?}", call);
                    let res = call.dispatch_bypass_filter(frame_system::RawOrigin::Root.into());
                    log!("dispatch_extrinsics_internal:: res {:?}", res);
                    (payload, res.is_ok())
                }
                _ => {
                    log!(
                        "dispatch_extrinsics_internal:: failed to decode extrinsic {}",
                        hex::encode(&payload)
                    );
                    (payload, false)
                }
            }
        })
        .collect();

    <Module<T>>::deposit_event(Event::ExecutedGovernance(results));

    Ok(())
}

pub fn lock_internal<T: Config>(
    asset: AssetInfo,
    holder: ChainAccount,
    amount: AssetQuantity,
) -> Result<(), Reason> {
    let holder_asset = AssetBalances::get(asset.asset, holder);
    let (holder_repay_amount, holder_supply_amount) = repay_and_supply_amount(holder_asset, amount);

    let holder_asset_new = add_amount_to_balance(holder_asset, amount)?;
    let total_supply_new =
        add_amount_to_raw(TotalSupplyAssets::get(asset.asset), holder_supply_amount)?;
    let total_borrow_new = sub_amount_from_raw(
        TotalBorrowAssets::get(asset.asset),
        holder_repay_amount,
        Reason::RepayTooMuch,
    )?;

    let (cash_principal_post, last_index_post) = effect_of_asset_interest_internal(
        asset,
        holder,
        holder_asset,
        holder_asset_new,
        CashPrincipals::get(holder),
    )?;

    LastIndices::insert(asset.asset, holder, last_index_post);
    CashPrincipals::insert(holder, cash_principal_post);
    TotalSupplyAssets::insert(asset.asset, total_supply_new);
    TotalBorrowAssets::insert(asset.asset, total_borrow_new);

    set_asset_balance_internal::<T>(asset.asset, holder, holder_asset_new);

    // XXX real events
    <Module<T>>::deposit_event(Event::GoldieLocks(asset.asset, holder, amount.value)); // XXX -> raw amount?

    Ok(()) // XXX events?
}

pub fn lock_cash_principal_internal<T: Config>(
    holder: ChainAccount,
    principal: CashPrincipalAmount,
) -> Result<(), Reason> {
    let holder_cash_principal = CashPrincipals::get(holder);
    let (holder_repay_principal, _holder_supply_principal) =
        repay_and_supply_principal(holder_cash_principal, principal);

    let chain_id = holder.chain_id();
    let chain_cash_principal_new = sub_principal_amounts(
        ChainCashPrincipals::get(chain_id),
        principal,
        Reason::InsufficientChainCash,
    )?;
    let holder_cash_principal_new = holder_cash_principal.add_amount(principal)?;
    let total_cash_principal_new = sub_principal_amounts(
        TotalCashPrincipal::get(),
        holder_repay_principal,
        Reason::RepayTooMuch,
    )?;

    ChainCashPrincipals::insert(chain_id, chain_cash_principal_new);
    CashPrincipals::insert(holder, holder_cash_principal_new);
    TotalCashPrincipal::put(total_cash_principal_new);

    <Module<T>>::deposit_event(Event::GoldieLocksCash(holder, principal)); // XXX -> raw amount?

    Ok(()) // XXX should we return events to be deposited?
}

pub fn extract_internal<T: Config>(
    asset: AssetInfo,
    holder: ChainAccount,
    recipient: ChainAccount,
    amount: AssetQuantity,
) -> Result<(), Reason> {
    let chain_asset_account =
        try_chain_asset_account(asset.asset, recipient).ok_or(Reason::ChainMismatch)?;
    require_min_tx_value!(get_value::<T>(amount)?);
    require!(
        has_liquidity_to_reduce_asset::<T>(holder, asset, amount)?,
        Reason::InsufficientLiquidity
    );

    let holder_asset = AssetBalances::get(asset.asset, holder);
    let (holder_withdraw_amount, holder_borrow_amount) =
        withdraw_and_borrow_amount(holder_asset, amount);

    let holder_asset_new = sub_amount_from_balance(holder_asset, amount)?;
    let total_supply_new = sub_amount_from_raw(
        TotalSupplyAssets::get(asset.asset),
        holder_withdraw_amount,
        Reason::InsufficientTotalFunds,
    )?;
    let total_borrow_new =
        add_amount_to_raw(TotalBorrowAssets::get(asset.asset), holder_borrow_amount)?;

    let (cash_principal_post, last_index_post) = effect_of_asset_interest_internal(
        asset,
        holder,
        holder_asset,
        holder_asset_new,
        CashPrincipals::get(holder),
    )?;

    LastIndices::insert(asset.asset, holder, last_index_post);
    CashPrincipals::insert(holder, cash_principal_post);
    TotalSupplyAssets::insert(asset.asset, total_supply_new);
    TotalBorrowAssets::insert(asset.asset, total_borrow_new);

    set_asset_balance_internal::<T>(asset.asset, holder, holder_asset_new);

    // XXX fix me this cannot fail
    dispatch_notice_internal::<T>(
        recipient.chain_id(),
        Some(recipient),
        &|notice_id, parent_hash| {
            Ok(Notice::ExtractionNotice(
                match (chain_asset_account, parent_hash) {
                    (
                        ChainAssetAccount::Eth(eth_asset, eth_account),
                        ChainHash::Eth(eth_parent_hash),
                    ) => Ok(ExtractionNotice::Eth {
                        id: notice_id,
                        parent: eth_parent_hash,
                        asset: eth_asset,
                        account: eth_account,
                        amount: amount.value,
                    }),
                    _ => Err(Reason::AssetExtractionNotSupported),
                }?,
            ))
        },
    )?;

    Ok(()) // XXX events?
}

pub fn extract_cash_principal_internal<T: Config>(
    holder: ChainAccount,
    recipient: ChainAccount,
    principal: CashPrincipalAmount,
) -> Result<(), Reason> {
    let index: CashIndex = GlobalCashIndex::get();
    let amount = index.cash_quantity(principal)?;

    require_min_tx_value!(get_value::<T>(amount)?);
    require!(
        has_liquidity_to_reduce_cash::<T>(holder, amount)?,
        Reason::InsufficientLiquidity
    );

    let holder_cash_principal = CashPrincipals::get(holder);
    let (_holder_withdraw_principal, holder_borrow_principal) =
        withdraw_and_borrow_principal(holder_cash_principal, principal);

    let chain_id = recipient.chain_id();
    let chain_cash_principal_new =
        add_principal_amounts(ChainCashPrincipals::get(chain_id), principal)?;
    let total_cash_principal_new =
        add_principal_amounts(TotalCashPrincipal::get(), holder_borrow_principal)?;
    let holder_cash_principal_new = holder_cash_principal.sub_amount(principal)?;

    ChainCashPrincipals::insert(chain_id, chain_cash_principal_new);
    CashPrincipals::insert(holder, holder_cash_principal_new);
    TotalCashPrincipal::put(total_cash_principal_new);

    // XXX fix me this cannot fail
    dispatch_notice_internal::<T>(
        recipient.chain_id(),
        Some(recipient),
        &|notice_id, parent_hash| {
            // XXX bound to eth?
            Ok(Notice::CashExtractionNotice(match (holder, parent_hash) {
                (ChainAccount::Eth(eth_account), ChainHash::Eth(eth_parent_hash)) => {
                    Ok(CashExtractionNotice::Eth {
                        id: notice_id,
                        parent: eth_parent_hash,
                        account: eth_account,
                        principal: principal.0,
                    })
                }
                _ => Err(Reason::AssetExtractionNotSupported),
            }?))
        },
    )?;

    Ok(()) // XXX events?
}

pub fn transfer_internal<T: Config>(
    asset: AssetInfo,
    sender: ChainAccount,
    recipient: ChainAccount,
    amount: AssetQuantity,
) -> Result<(), Reason> {
    let miner = get_some_miner::<T>();
    let index = GlobalCashIndex::get();

    // XXX check asset matches amount asset?
    require!(sender != recipient, Reason::SelfTransfer);
    require_min_tx_value!(get_value::<T>(amount)?);
    require!(
        has_liquidity_to_reduce_asset_with_fee::<T>(sender, asset, amount, TRANSFER_FEE)?,
        Reason::InsufficientLiquidity
    );

    let sender_asset = AssetBalances::get(asset.asset, sender);
    let recipient_asset = AssetBalances::get(asset.asset, recipient);
    let sender_cash_principal = CashPrincipals::get(sender);
    let miner_cash_principal = CashPrincipals::get(miner);

    let fee_principal = index.cash_principal_amount(TRANSFER_FEE)?;
    let (sender_withdraw_amount, sender_borrow_amount) =
        withdraw_and_borrow_amount(sender_asset, amount);
    let (recipient_repay_amount, recipient_supply_amount) =
        repay_and_supply_amount(recipient_asset, amount);
    let (_sender_withdraw_principal, sender_borrow_principal) =
        withdraw_and_borrow_principal(sender_cash_principal, fee_principal);
    let (miner_repay_principal, _miner_supply_principal) =
        repay_and_supply_principal(miner_cash_principal, fee_principal);

    let miner_cash_principal_new = miner_cash_principal.add_amount(fee_principal)?;
    let sender_cash_principal_new = sender_cash_principal.sub_amount(fee_principal)?;
    let sender_asset_new = sub_amount_from_balance(sender_asset, amount)?;
    let recipient_asset_new = add_amount_to_balance(recipient_asset, amount)?;

    let total_supply_new = sub_amount_from_raw(
        add_amount_to_raw(TotalSupplyAssets::get(asset.asset), recipient_supply_amount)?,
        sender_withdraw_amount,
        Reason::InsufficientTotalFunds,
    )?;
    let total_borrow_new = sub_amount_from_raw(
        add_amount_to_raw(TotalBorrowAssets::get(asset.asset), sender_borrow_amount)?,
        recipient_repay_amount,
        Reason::RepayTooMuch,
    )?;
    let total_cash_principal_new = sub_principal_amounts(
        add_principal_amounts(TotalCashPrincipal::get(), sender_borrow_principal)?,
        miner_repay_principal,
        Reason::RepayTooMuch,
    )?;

    let (sender_cash_principal_post, sender_last_index_post) = effect_of_asset_interest_internal(
        asset,
        sender,
        sender_asset,
        sender_asset_new,
        sender_cash_principal_new,
    )?;
    let (recipient_cash_principal_post, recipient_last_index_post) =
        effect_of_asset_interest_internal(
            asset,
            recipient,
            recipient_asset,
            recipient_asset_new,
            CashPrincipals::get(recipient),
        )?;

    LastIndices::insert(asset.asset, sender, sender_last_index_post);
    LastIndices::insert(asset.asset, recipient, recipient_last_index_post);
    CashPrincipals::insert(sender, sender_cash_principal_post);
    CashPrincipals::insert(recipient, recipient_cash_principal_post);
    CashPrincipals::insert(miner, miner_cash_principal_new);
    TotalSupplyAssets::insert(asset.asset, total_supply_new);
    TotalBorrowAssets::insert(asset.asset, total_borrow_new);
    TotalCashPrincipal::put(total_cash_principal_new);

    set_asset_balance_internal::<T>(asset.asset, sender, sender_asset_new);
    set_asset_balance_internal::<T>(asset.asset, recipient, recipient_asset_new);

    Ok(()) // XXX events?
}

pub fn transfer_cash_principal_internal<T: Config>(
    sender: ChainAccount,
    recipient: ChainAccount,
    principal: CashPrincipalAmount,
) -> Result<(), Reason> {
    let miner = get_some_miner::<T>();
    let index: CashIndex = GlobalCashIndex::get();
    let amount = index.cash_quantity(principal)?;

    require!(sender != recipient, Reason::SelfTransfer);
    require_min_tx_value!(get_value::<T>(amount)?);
    require!(
        has_liquidity_to_reduce_cash::<T>(sender, amount.add(TRANSFER_FEE)?)?,
        Reason::InsufficientLiquidity
    );

    let sender_cash_principal = CashPrincipals::get(sender);
    let recipient_cash_principal = CashPrincipals::get(recipient);
    let miner_cash_principal = CashPrincipals::get(miner);

    let fee_principal = index.cash_principal_amount(TRANSFER_FEE)?;
    let principal_with_fee = principal.add(fee_principal)?;
    let (_sender_withdraw_principal, sender_borrow_principal) =
        withdraw_and_borrow_principal(sender_cash_principal, principal_with_fee);
    let (recipient_repay_principal, _recipient_supply_principal) =
        repay_and_supply_principal(recipient_cash_principal, principal);
    let (miner_repay_principal, _miner_supply_principal) =
        repay_and_supply_principal(miner_cash_principal, fee_principal);

    let miner_cash_principal_new = miner_cash_principal.add_amount(fee_principal)?;
    let sender_cash_principal_new = sender_cash_principal.sub_amount(principal_with_fee)?;
    let recipient_cash_principal_new = recipient_cash_principal.add_amount(principal)?;

    let total_cash_principal_new = sub_principal_amounts(
        add_principal_amounts(TotalCashPrincipal::get(), sender_borrow_principal)?,
        add_principal_amounts(recipient_repay_principal, miner_repay_principal)?,
        Reason::RepayTooMuch,
    )?;

    CashPrincipals::insert(miner, miner_cash_principal_new);
    CashPrincipals::insert(sender, sender_cash_principal_new);
    CashPrincipals::insert(recipient, recipient_cash_principal_new);
    TotalCashPrincipal::put(total_cash_principal_new);

    Ok(()) // XXX events?
}

pub fn liquidate_internal<T: Config>(
    asset: AssetInfo,
    collateral_asset: AssetInfo,
    liquidator: ChainAccount,
    borrower: ChainAccount,
    amount: AssetQuantity,
) -> Result<(), Reason> {
    require!(borrower != liquidator, Reason::SelfTransfer);
    require!(asset != collateral_asset, Reason::InKindLiquidation);
    require_min_tx_value!(get_value::<T>(amount)?);

    let liquidation_incentive = Factor::from_nominal("1.08"); // XXX spec first
    let liquidator_asset = AssetBalances::get(asset.asset, liquidator);
    let borrower_asset = AssetBalances::get(asset.asset, borrower);
    let liquidator_collateral_asset = AssetBalances::get(collateral_asset.asset, liquidator);
    let borrower_collateral_asset = AssetBalances::get(collateral_asset.asset, borrower);
    let seize_amount = amount
        .mul_factor(liquidation_incentive)?
        .mul_price(get_price::<T>(asset.units())?)?
        .div_price(
            get_price::<T>(collateral_asset.units())?,
            collateral_asset.units(),
        )?;

    require!(
        has_liquidity_to_reduce_asset_with_added_collateral::<T>(
            liquidator,
            asset,
            amount,
            collateral_asset,
            seize_amount,
        )?,
        Reason::InsufficientLiquidity
    );

    let (borrower_repay_amount, _borrower_supply_amount) =
        repay_and_supply_amount(liquidator_asset, amount);
    let (liquidator_withdraw_amount, liquidator_borrow_amount) =
        withdraw_and_borrow_amount(borrower_asset, amount);
    let (borrower_collateral_withdraw_amount, _borrower_collateral_borrow_amount) =
        withdraw_and_borrow_amount(borrower_collateral_asset, seize_amount);
    let (liquidator_collateral_repay_amount, liquidator_collateral_supply_amount) =
        repay_and_supply_amount(liquidator_collateral_asset, seize_amount);

    let borrower_asset_new = add_amount_to_balance(borrower_asset, amount)?;
    let liquidator_asset_new = sub_amount_from_balance(liquidator_asset, amount)?;
    let borrower_collateral_asset_new =
        sub_amount_from_balance(borrower_collateral_asset, seize_amount)?;
    let liquidator_collateral_asset_new =
        add_amount_to_balance(liquidator_collateral_asset, seize_amount)?;

    let total_supply_new = sub_amount_from_raw(
        TotalSupplyAssets::get(asset.asset),
        liquidator_withdraw_amount,
        Reason::InsufficientTotalFunds,
    )?;
    let total_borrow_new = sub_amount_from_raw(
        add_amount_to_raw(
            TotalBorrowAssets::get(asset.asset),
            liquidator_borrow_amount,
        )?,
        borrower_repay_amount,
        Reason::RepayTooMuch,
    )?;
    let total_collateral_supply_new = sub_amount_from_raw(
        add_amount_to_raw(
            TotalSupplyAssets::get(collateral_asset.asset),
            liquidator_collateral_supply_amount,
        )?,
        borrower_collateral_withdraw_amount,
        Reason::InsufficientTotalFunds,
    )?;
    let total_collateral_borrow_new = sub_amount_from_raw(
        TotalBorrowAssets::get(collateral_asset.asset),
        liquidator_collateral_repay_amount,
        Reason::RepayTooMuch,
    )?;

    let (borrower_cash_principal_post, borrower_last_index_post) =
        effect_of_asset_interest_internal(
            asset,
            borrower,
            borrower_asset,
            borrower_asset_new,
            CashPrincipals::get(borrower),
        )?;
    let (liquidator_cash_principal_post, liquidator_last_index_post) =
        effect_of_asset_interest_internal(
            asset,
            liquidator,
            liquidator_asset,
            liquidator_asset_new,
            CashPrincipals::get(liquidator),
        )?;
    let (borrower_cash_principal_post, borrower_collateral_last_index_post) =
        effect_of_asset_interest_internal(
            collateral_asset,
            borrower,
            borrower_collateral_asset,
            borrower_collateral_asset_new,
            borrower_cash_principal_post,
        )?;
    let (liquidator_cash_principal_post, liquidator_collateral_last_index_post) =
        effect_of_asset_interest_internal(
            collateral_asset,
            liquidator,
            liquidator_collateral_asset,
            liquidator_collateral_asset_new,
            liquidator_cash_principal_post,
        )?;

    LastIndices::insert(asset.asset, borrower, borrower_last_index_post);
    LastIndices::insert(asset.asset, liquidator, liquidator_last_index_post);
    LastIndices::insert(
        collateral_asset.asset,
        borrower,
        borrower_collateral_last_index_post,
    );
    LastIndices::insert(
        collateral_asset.asset,
        liquidator,
        liquidator_collateral_last_index_post,
    );
    CashPrincipals::insert(borrower, borrower_cash_principal_post);
    CashPrincipals::insert(liquidator, liquidator_cash_principal_post);
    TotalSupplyAssets::insert(asset.asset, total_supply_new);
    TotalBorrowAssets::insert(asset.asset, total_borrow_new);
    TotalSupplyAssets::insert(collateral_asset.asset, total_collateral_supply_new);
    TotalBorrowAssets::insert(collateral_asset.asset, total_collateral_borrow_new);

    set_asset_balance_internal::<T>(asset.asset, borrower, borrower_asset_new);
    set_asset_balance_internal::<T>(asset.asset, liquidator, liquidator_asset_new);
    set_asset_balance_internal::<T>(
        collateral_asset.asset,
        borrower,
        borrower_collateral_asset_new,
    );
    set_asset_balance_internal::<T>(
        collateral_asset.asset,
        liquidator,
        liquidator_collateral_asset_new,
    );

    Ok(()) // XXX events?
}

pub fn liquidate_cash_principal_internal<T: Config>(
    collateral_asset: AssetInfo,
    liquidator: ChainAccount,
    borrower: ChainAccount,
    principal: CashPrincipalAmount,
) -> Result<(), Reason> {
    let index = GlobalCashIndex::get();
    let amount = index.cash_quantity(principal)?;

    require!(borrower != liquidator, Reason::SelfTransfer);
    require_min_tx_value!(get_value::<T>(amount)?);

    let liquidation_incentive = Factor::from_nominal("1.08"); // XXX spec first
    let liquidator_cash_principal = CashPrincipals::get(liquidator);
    let borrower_cash_principal = CashPrincipals::get(borrower);
    let liquidator_collateral_asset = AssetBalances::get(collateral_asset.asset, liquidator);
    let borrower_collateral_asset = AssetBalances::get(collateral_asset.asset, borrower);
    let seize_amount = amount
        .mul_factor(liquidation_incentive)?
        .mul_price(get_price::<T>(CASH)?)?
        .div_price(
            get_price::<T>(collateral_asset.units())?,
            collateral_asset.units(),
        )?;

    require!(
        has_liquidity_to_reduce_cash_with_added_collateral::<T>(
            liquidator,
            amount,
            collateral_asset,
            seize_amount,
        )?,
        Reason::InsufficientLiquidity
    );

    let (borrower_repay_principal, _borrower_supply_principal) =
        repay_and_supply_principal(liquidator_cash_principal, principal);
    let (_liquidator_withdraw_principal, liquidator_borrow_principal) =
        withdraw_and_borrow_principal(borrower_cash_principal, principal);
    let (borrower_collateral_withdraw_amount, _borrower_collateral_borrow_amount) =
        withdraw_and_borrow_amount(borrower_collateral_asset, seize_amount);
    let (liquidator_collateral_repay_amount, liquidator_collateral_supply_amount) =
        repay_and_supply_amount(liquidator_collateral_asset, seize_amount);

    let borrower_cash_principal_new = borrower_cash_principal.add_amount(principal)?;
    let liquidator_cash_principal_new = liquidator_cash_principal.sub_amount(principal)?;
    let borrower_collateral_asset_new =
        sub_amount_from_balance(borrower_collateral_asset, seize_amount)?;
    let liquidator_collateral_asset_new =
        add_amount_to_balance(liquidator_collateral_asset, seize_amount)?;

    let total_cash_principal_new = sub_principal_amounts(
        add_principal_amounts(TotalCashPrincipal::get(), liquidator_borrow_principal)?,
        borrower_repay_principal,
        Reason::RepayTooMuch,
    )?;
    let total_collateral_supply_new = sub_amount_from_raw(
        add_amount_to_raw(
            TotalSupplyAssets::get(collateral_asset.asset),
            liquidator_collateral_supply_amount,
        )?,
        borrower_collateral_withdraw_amount,
        Reason::InsufficientTotalFunds,
    )?;
    let total_collateral_borrow_new = sub_amount_from_raw(
        TotalBorrowAssets::get(collateral_asset.asset),
        liquidator_collateral_repay_amount,
        Reason::RepayTooMuch,
    )?;

    let (borrower_cash_principal_post, borrower_collateral_last_index_post) =
        effect_of_asset_interest_internal(
            collateral_asset,
            borrower,
            borrower_collateral_asset,
            borrower_collateral_asset_new,
            borrower_cash_principal_new,
        )?;
    let (liquidator_cash_principal_post, liquidator_collateral_last_index_post) =
        effect_of_asset_interest_internal(
            collateral_asset,
            liquidator,
            liquidator_collateral_asset,
            liquidator_collateral_asset_new,
            liquidator_cash_principal_new,
        )?;

    LastIndices::insert(
        collateral_asset.asset,
        borrower,
        borrower_collateral_last_index_post,
    );
    LastIndices::insert(
        collateral_asset.asset,
        liquidator,
        liquidator_collateral_last_index_post,
    );
    CashPrincipals::insert(borrower, borrower_cash_principal_post);
    CashPrincipals::insert(liquidator, liquidator_cash_principal_post);
    TotalCashPrincipal::put(total_cash_principal_new);
    TotalSupplyAssets::insert(collateral_asset.asset, total_collateral_supply_new);
    TotalBorrowAssets::insert(collateral_asset.asset, total_collateral_borrow_new);

    set_asset_balance_internal::<T>(
        collateral_asset.asset,
        borrower,
        borrower_collateral_asset_new,
    );
    set_asset_balance_internal::<T>(
        collateral_asset.asset,
        liquidator,
        liquidator_collateral_asset_new,
    );

    Ok(()) // XXX events?
}

pub fn liquidate_cash_collateral_internal<T: Config>(
    asset: AssetInfo,
    liquidator: ChainAccount,
    borrower: ChainAccount,
    amount: AssetQuantity,
) -> Result<(), Reason> {
    let index = GlobalCashIndex::get();

    require!(borrower != liquidator, Reason::SelfTransfer);
    require_min_tx_value!(get_value::<T>(amount)?);

    let liquidation_incentive = Factor::from_nominal("1.08"); // XXX spec first
    let liquidator_asset = AssetBalances::get(asset.asset, liquidator);
    let borrower_asset = AssetBalances::get(asset.asset, borrower);
    let liquidator_cash_principal = CashPrincipals::get(liquidator);
    let borrower_cash_principal = CashPrincipals::get(borrower);
    let seize_amount = amount
        .mul_factor(liquidation_incentive)?
        .mul_price(get_price::<T>(asset.units())?)?
        .div_price(get_price::<T>(CASH)?, CASH)?;
    let seize_principal = index.cash_principal_amount(seize_amount)?;

    require!(
        has_liquidity_to_reduce_asset_with_added_cash::<T>(
            liquidator,
            asset,
            amount,
            seize_amount
        )?,
        Reason::InsufficientLiquidity
    );

    let (borrower_repay_amount, _borrower_supply_amount) =
        repay_and_supply_amount(liquidator_asset, amount);
    let (liquidator_withdraw_amount, liquidator_borrow_amount) =
        withdraw_and_borrow_amount(borrower_asset, amount);
    let (borrower_collateral_withdraw_principal, _borrower_collateral_borrow_principal) =
        withdraw_and_borrow_principal(borrower_cash_principal, seize_principal);
    let (liquidator_collateral_repay_principal, _liquidator_collateral_supply_principal) =
        repay_and_supply_principal(
            liquidator_cash_principal,
            borrower_collateral_withdraw_principal,
        );

    let borrower_asset_new = add_amount_to_balance(borrower_asset, amount)?;
    let liquidator_asset_new = sub_amount_from_balance(liquidator_asset, amount)?;
    let borrower_cash_principal_new = borrower_cash_principal.sub_amount(seize_principal)?;
    let liquidator_cash_principal_new = liquidator_cash_principal.add_amount(seize_principal)?;

    let total_supply_new = sub_amount_from_raw(
        TotalSupplyAssets::get(asset.asset),
        liquidator_withdraw_amount,
        Reason::InsufficientTotalFunds,
    )?;
    let total_borrow_new = sub_amount_from_raw(
        add_amount_to_raw(
            TotalBorrowAssets::get(asset.asset),
            liquidator_borrow_amount,
        )?,
        borrower_repay_amount,
        Reason::RepayTooMuch,
    )?;
    let total_cash_principal_new = sub_principal_amounts(
        TotalCashPrincipal::get(),
        liquidator_collateral_repay_principal,
        Reason::RepayTooMuch,
    )?;

    let (borrower_cash_principal_post, borrower_last_index_post) =
        effect_of_asset_interest_internal(
            asset,
            borrower,
            borrower_asset,
            borrower_asset_new,
            borrower_cash_principal_new,
        )?;
    let (liquidator_cash_principal_post, liquidator_last_index_post) =
        effect_of_asset_interest_internal(
            asset,
            liquidator,
            liquidator_asset,
            liquidator_asset_new,
            liquidator_cash_principal_new,
        )?;

    LastIndices::insert(asset.asset, borrower, borrower_last_index_post);
    LastIndices::insert(asset.asset, liquidator, liquidator_last_index_post);
    CashPrincipals::insert(borrower, borrower_cash_principal_post);
    CashPrincipals::insert(liquidator, liquidator_cash_principal_post);
    TotalSupplyAssets::insert(asset.asset, total_supply_new);
    TotalBorrowAssets::insert(asset.asset, total_borrow_new);
    TotalCashPrincipal::put(total_cash_principal_new);

    set_asset_balance_internal::<T>(asset.asset, borrower, borrower_asset_new);
    set_asset_balance_internal::<T>(asset.asset, liquidator, liquidator_asset_new);

    Ok(()) // XXX events?
}

// Liquidity Checks //

/// Calculates if an account will remain solvent after reducing asset by amount.
pub fn has_liquidity_to_reduce_asset<T: Config>(
    account: ChainAccount,
    asset: AssetInfo,
    amount: AssetQuantity,
) -> Result<bool, Reason> {
    let liquidity = Portfolio::from_storage::<T>(account)?
        .asset_change(asset, amount.as_decrease()?)?
        .get_liquidity::<T>()?;
    Ok(liquidity.value >= 0)
}

/// Calculates if an account will remain solvent after reducing asset by amount and paying a CASH fee.
pub fn has_liquidity_to_reduce_asset_with_fee<T: Config>(
    account: ChainAccount,
    asset: AssetInfo,
    amount: AssetQuantity,
    fee: CashQuantity,
) -> Result<bool, Reason> {
    let liquidity = Portfolio::from_storage::<T>(account)?
        .asset_change(asset, amount.as_decrease()?)?
        .cash_change(fee.as_decrease()?)?
        .get_liquidity::<T>()?;
    Ok(liquidity.value >= 0)
}

/// Calculates if an account will remain solvent after reducing asset by amount and adding an amount of asset collateral.
pub fn has_liquidity_to_reduce_asset_with_added_collateral<T: Config>(
    account: ChainAccount,
    asset: AssetInfo,
    amount: AssetQuantity,
    collateral_asset: AssetInfo,
    collateral_amount: AssetQuantity,
) -> Result<bool, Reason> {
    let liquidity = Portfolio::from_storage::<T>(account)?
        .asset_change(asset, amount.as_decrease()?)?
        .asset_change(collateral_asset, collateral_amount.as_increase()?)?
        .get_liquidity::<T>()?;
    Ok(liquidity.value >= 0)
}

/// Calculates if an account will remain solvent after reducing asset by amount and adding an amount of CASH collateral.
pub fn has_liquidity_to_reduce_asset_with_added_cash<T: Config>(
    account: ChainAccount,
    asset: AssetInfo,
    amount: AssetQuantity,
    cash_amount: CashQuantity,
) -> Result<bool, Reason> {
    let liquidity = Portfolio::from_storage::<T>(account)?
        .asset_change(asset, amount.as_decrease()?)?
        .cash_change(cash_amount.as_increase()?)?
        .get_liquidity::<T>()?;
    Ok(liquidity.value >= 0)
}

/// Calculates if an account will remain solvent after reducing CASH by amount.
pub fn has_liquidity_to_reduce_cash<T: Config>(
    account: ChainAccount,
    amount: CashQuantity,
) -> Result<bool, Reason> {
    let portfolio = Portfolio::from_storage::<T>(account)?;
    let liquidity = portfolio
        .cash_change(amount.as_decrease()?)?
        .get_liquidity::<T>()?;
    Ok(liquidity.value >= 0)
}

/// Calculates if an account will remain solvent after reducing CASH by amount and adding an amount of asset collateral.
pub fn has_liquidity_to_reduce_cash_with_added_collateral<T: Config>(
    account: ChainAccount,
    amount: CashQuantity,
    collateral_asset: AssetInfo,
    collateral_amount: AssetQuantity,
) -> Result<bool, Reason> {
    let liquidity = Portfolio::from_storage::<T>(account)?
        .cash_change(amount.as_decrease()?)?
        .asset_change(collateral_asset, collateral_amount.as_increase()?)?
        .get_liquidity::<T>()?;
    Ok(liquidity.value >= 0)
}

/// Calculates the current liquidity value for an account.
pub fn get_liquidity<T: Config>(account: ChainAccount) -> Result<Balance, Reason> {
    Ok(Portfolio::from_storage::<T>(account)?.get_liquidity::<T>()?)
}

/// Calculates the current CASH principal of the account, including all interest from non-CASH markets.
pub fn get_cash_principal_with_asset_interest<T: Config>(
    account: ChainAccount,
) -> Result<CashPrincipal, Reason> {
    let mut principal = CashPrincipals::get(account);
    for (asset, _) in AssetsWithNonZeroBalance::iter_prefix(account) {
        let asset_info = get_asset::<T>(asset)?;
        let balance = AssetBalances::get(asset, account);
        (principal, _) =
            effect_of_asset_interest_internal(asset_info, account, balance, balance, principal)?;
    }
    Ok(principal)
}

/// Calculates the current total CASH value of the account, including all interest from non-CASH markets.
pub fn get_cash_balance_with_asset_interest<T: Config>(
    account: ChainAccount,
) -> Result<Balance, Reason> {
    Ok(GlobalCashIndex::get()
        .cash_balance(get_cash_principal_with_asset_interest::<T>(account)?)?)
}

// Asset Interest //

/// Return CASH Principal post asset interest, and updated asset index, for a given account
pub fn effect_of_asset_interest_internal(
    asset_info: AssetInfo,
    account: ChainAccount,
    asset_balance_old: AssetBalance,
    asset_balance_new: AssetBalance,
    cash_principal_pre: CashPrincipal,
) -> Result<(CashPrincipal, AssetIndex), MathError> {
    let asset = asset_info.asset;
    let last_index = LastIndices::get(asset, account);
    let cash_index = if asset_balance_old >= 0 {
        SupplyIndices::get(asset)
    } else {
        BorrowIndices::get(asset)
    };
    let balance_old = asset_info.as_balance(asset_balance_old);
    let cash_principal_delta = cash_index.cash_principal_since(last_index, balance_old)?;
    let cash_principal_post = cash_principal_pre.add(cash_principal_delta)?;
    let last_index_post = if asset_balance_new >= 0 {
        SupplyIndices::get(asset)
    } else {
        BorrowIndices::get(asset)
    };
    Ok((cash_principal_post, last_index_post))
}

// Dispatch Extrinsic Lifecycle //

/// Block initialization wrapper.
// XXX we need to be able to mock Now (then get rid of this?)
pub fn on_initialize<T: Config>() -> Result<frame_support::weights::Weight, Reason> {
    on_initialize_internal::<T>(get_now::<T>(), LastYieldTimestamp::get())
}

/// Block initialization step that can fail.
pub fn on_initialize_internal<T: Config>(
    now: Timestamp,
    last_yield_timestamp: Timestamp,
) -> Result<frame_support::weights::Weight, Reason> {
    // xxx re-evaluate how we do time, we don't really want this to be zero but there may
    // not actually be any good way to do "current" time per-se so what we have here is more like
    // the last block's time and the block before
    // XXX also we should try to inject Now (and previous) for tests instead of taking different path
    if now == 0 {
        return Err(Reason::TimeTravelNotAllowed);
    }
    if last_yield_timestamp == 0 {
        // this is the first time we have seen a valid time, set it
        LastYieldTimestamp::put(now);
        return Ok(0);
    }

    // Iterate through listed assets, summing the CASH principal they generated/paid last block
    let dt = now
        .checked_sub(last_yield_timestamp)
        .ok_or(Reason::TimeTravelNotAllowed)?;
    let mut cash_principal_supply_increase = CashPrincipalAmount::ZERO;
    let mut cash_principal_borrow_increase = CashPrincipalAmount::ZERO;

    let cash_index = GlobalCashIndex::get();
    let cash_yield = CashYield::get();
    let price_cash = get_price::<T>(CASH)?;

    let mut asset_updates: Vec<(ChainAsset, AssetIndex, AssetIndex)> = Vec::new();
    for (asset, asset_info) in SupportedAssets::iter() {
        let (asset_cost, asset_yield) = crate::core::get_rates::<T>(asset)?;
        let asset_units = asset_info.units();
        let price_asset = get_price::<T>(asset_units)?;
        let price_ratio = price_asset.ratio(price_cash)?;
        let cash_borrow_principal_per_asset =
            cash_index.cash_principal_per_asset(asset_cost.simple(dt)?, price_ratio)?;
        let cash_hold_principal_per_asset =
            cash_index.cash_principal_per_asset(asset_yield.simple(dt)?, price_ratio)?;

        let supply_index = SupplyIndices::get(&asset);
        let borrow_index = BorrowIndices::get(&asset);
        let supply_index_new = supply_index.increment(cash_hold_principal_per_asset)?;
        let borrow_index_new = borrow_index.increment(cash_borrow_principal_per_asset)?;

        let supply_asset = Quantity::new(TotalSupplyAssets::get(asset), asset_units);
        let borrow_asset = Quantity::new(TotalBorrowAssets::get(asset), asset_units);
        cash_principal_supply_increase = cash_principal_supply_increase
            .add(cash_hold_principal_per_asset.cash_principal_amount(supply_asset)?)?;
        cash_principal_borrow_increase = cash_principal_borrow_increase
            .add(cash_borrow_principal_per_asset.cash_principal_amount(borrow_asset)?)?;

        asset_updates.push((asset.clone(), supply_index_new, borrow_index_new));
    }

    // Pay miners and update the CASH interest index on CASH itself
    if cash_yield == APR::ZERO {
        log!("Cash yield is zero. No interest earned on cash in this block.");
    }

    let total_cash_principal = TotalCashPrincipal::get();

    let increment = cash_yield.compound(dt)?;
    if increment == CashIndex::ONE {
        log!("Index increment = 1. No interest on cash earned in this block!")
    }
    let cash_index_new = cash_index.increment(increment)?; // XXX
    let total_cash_principal_new = total_cash_principal.add(cash_principal_borrow_increase)?;
    let miner_share_principal =
        cash_principal_borrow_increase.sub(cash_principal_supply_increase)?;

    let last_miner = get_some_miner::<T>(); // Miner not yet set for this block, so this is "last miner"
    let last_miner_share_principal = LastMinerSharePrincipal::get();
    let miner_cash_principal_old = CashPrincipals::get(&last_miner);
    let miner_cash_principal_new =
        miner_cash_principal_old.add_amount(last_miner_share_principal)?;

    // * BEGIN STORAGE ALL CHECKS AND FAILURES MUST HAPPEN ABOVE * //

    CashPrincipals::insert(last_miner, miner_cash_principal_new);
    log!(
        "Miner={:?} received {:?} principal for mining last block",
        String::from(last_miner),
        last_miner_share_principal
    );

    for (asset, new_supply_index, new_borrow_index) in asset_updates.drain(..) {
        SupplyIndices::insert(asset.clone(), new_supply_index);
        BorrowIndices::insert(asset, new_borrow_index);
    }

    GlobalCashIndex::put(cash_index_new);
    TotalCashPrincipal::put(total_cash_principal_new);
    LastMinerSharePrincipal::put(miner_share_principal);
    LastYieldTimestamp::put(now);

    // Possibly rotate in any scheduled next CASH rate
    if let Some((next_apr, next_start)) = CashYieldNext::get() {
        if next_start <= now {
            CashYield::put(next_apr);
            CashYieldNext::kill();
        }
    }

    Ok(0) // XXX weight
}

#[cfg(test)]
mod tests {
    use crate::{
        chains::*, core::*, mock::*, params::*, rates::*, symbol::*, tests::*, types::*, *,
    };
    use our_std::str::FromStr;

    #[test]
    fn test_helpers() {
        assert_eq!(neg_balance(100), 0);
        assert_eq!(pos_balance(-100), 0);
        assert_eq!(neg_balance(-100), 100);
        assert_eq!(pos_balance(100), 100);

        let amount = Quantity::new(100, USD);
        assert_eq!(
            repay_and_supply_amount(0, amount),
            (Quantity::new(0, USD), amount)
        );
        assert_eq!(
            repay_and_supply_amount(-50, amount),
            (Quantity::new(50, USD), Quantity::new(50, USD))
        );
        assert_eq!(
            repay_and_supply_amount(-100, amount),
            (amount, Quantity::new(0, USD))
        );
        assert_eq!(
            withdraw_and_borrow_amount(0, amount),
            (Quantity::new(0, USD), amount)
        );
        assert_eq!(
            withdraw_and_borrow_amount(50, amount),
            (Quantity::new(50, USD), Quantity::new(50, USD))
        );
        assert_eq!(
            withdraw_and_borrow_amount(100, amount),
            (amount, Quantity::new(0, USD))
        );

        let principal = CashPrincipalAmount(100);
        assert_eq!(
            repay_and_supply_principal(CashPrincipal(0), principal),
            (CashPrincipalAmount(0), principal)
        );
        assert_eq!(
            repay_and_supply_principal(CashPrincipal(-50), principal),
            (CashPrincipalAmount(50), CashPrincipalAmount(50))
        );
        assert_eq!(
            repay_and_supply_principal(CashPrincipal(-100), principal),
            (principal, CashPrincipalAmount(0))
        );
        assert_eq!(
            withdraw_and_borrow_principal(CashPrincipal(0), principal),
            (CashPrincipalAmount(0), principal)
        );
        assert_eq!(
            withdraw_and_borrow_principal(CashPrincipal(50), principal),
            (CashPrincipalAmount(50), CashPrincipalAmount(50))
        );
        assert_eq!(
            withdraw_and_borrow_principal(CashPrincipal(100), principal),
            (principal, CashPrincipalAmount(0))
        );
    }

    #[test]
    fn test_extract_internal_min_value() {
        let asset = ChainAsset::Eth([238; 20]);
        let asset_info = AssetInfo::minimal(asset, ETH);
        let holder = ChainAccount::Eth([0; 20]);
        let recipient = ChainAccount::Eth([0; 20]);

        new_test_ext().execute_with(|| {
            SupportedAssets::insert(&asset, asset_info);
            Prices::insert(asset_info.ticker, 100_000); // $0.10
            let quantity = get_quantity::<Test>(asset, 5_000_000_000_000_000_000).unwrap();
            let asset_balances_pre = AssetBalances::get(asset, holder);
            let total_supply_pre = TotalSupplyAssets::get(asset);
            let total_borrows_pre = TotalBorrowAssets::get(asset);
            let events_pre: Vec<_> = System::events().into_iter().collect();
            let notices_pre: Vec<(NoticeId, Notice)> = Notices::iter_prefix(ChainId::Eth).collect();
            let notice_states_pre: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let latest_notice_pre: Option<(NoticeId, ChainHash)> = LatestNotice::get(ChainId::Eth);
            let notice_hashes_pre: Vec<(ChainHash, NoticeId)> = NoticeHashes::iter().collect();
            let account_notices_pre: Vec<(ChainAccount, Vec<NoticeId>)> =
                AccountNotices::iter().collect();

            let res = extract_internal::<Test>(asset_info, holder, recipient, quantity);

            assert_eq!(res, Err(Reason::MinTxValueNotMet));

            let asset_balances_post = AssetBalances::get(asset, holder);
            let total_supply_post = TotalSupplyAssets::get(asset);
            let total_borrows_post = TotalBorrowAssets::get(asset);
            let events_post: Vec<_> = System::events().into_iter().collect();
            let notices_post: Vec<(NoticeId, Notice)> =
                Notices::iter_prefix(ChainId::Eth).collect();
            let notice_states_post: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let latest_notice_post: Option<(NoticeId, ChainHash)> = LatestNotice::get(ChainId::Eth);
            let notice_hashes_post: Vec<(ChainHash, NoticeId)> = NoticeHashes::iter().collect();
            let account_notices_post: Vec<(ChainAccount, Vec<NoticeId>)> =
                AccountNotices::iter().collect();

            assert_eq!(asset_balances_pre, asset_balances_post);
            assert_eq!(total_supply_pre, total_supply_post);
            assert_eq!(total_borrows_pre, total_borrows_post);
            assert_eq!(events_pre.len(), events_post.len());
            assert_eq!(notices_pre, notices_post);
            assert_eq!(notice_states_pre, notice_states_post);
            assert_eq!(latest_notice_pre, latest_notice_post);
            assert_eq!(notice_hashes_pre, notice_hashes_post);
            assert_eq!(account_notices_pre, account_notices_post);
        });
    }

    #[test]
    fn test_extract_internal_sufficient_value() {
        let eth_asset = [238; 20];
        let asset = ChainAsset::Eth(eth_asset);
        let asset_info = AssetInfo {
            liquidity_factor: LiquidityFactor::from_nominal("1"),
            ..AssetInfo::minimal(asset, ETH)
        };
        let eth_holder = [0; 20];
        let eth_recipient = [0; 20];
        let holder = ChainAccount::Eth(eth_holder);
        let recipient = ChainAccount::Eth(eth_recipient);

        new_test_ext().execute_with(|| {
            SupportedAssets::insert(&asset, asset_info);
            Prices::insert(asset_info.ticker, 100_000); // $0.10
            let quantity = get_quantity::<Test>(asset, 50_000_000_000_000_000_000).unwrap();
            let hodl_balance = quantity.value * 5;
            AssetBalances::insert(asset, holder, hodl_balance as AssetBalance);
            AssetsWithNonZeroBalance::insert(holder, asset, ());
            TotalSupplyAssets::insert(&asset, hodl_balance);

            let asset_balances_pre = AssetBalances::get(asset, holder);
            let total_supply_pre = TotalSupplyAssets::get(asset);
            let total_borrows_pre = TotalBorrowAssets::get(asset);
            let events_pre: Vec<_> = System::events().into_iter().collect();
            let notices_pre: Vec<(NoticeId, Notice)> = Notices::iter_prefix(ChainId::Eth).collect();
            let notice_states_pre: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let latest_notice_pre: Option<(NoticeId, ChainHash)> = LatestNotice::get(ChainId::Eth);
            let notice_hashes_pre: Vec<(ChainHash, NoticeId)> = NoticeHashes::iter().collect();
            let account_notices_pre: Vec<(ChainAccount, Vec<NoticeId>)> =
                AccountNotices::iter().collect();

            let res = extract_internal::<Test>(asset_info, holder, recipient, quantity);

            assert_eq!(res, Ok(()));

            let asset_balances_post = AssetBalances::get(asset, holder);
            let total_supply_post = TotalSupplyAssets::get(asset);
            let total_borrows_post = TotalBorrowAssets::get(asset);
            let events_post: Vec<_> = System::events().into_iter().collect();
            let notices_post: Vec<(NoticeId, Notice)> =
                Notices::iter_prefix(ChainId::Eth).collect();
            let notice_states_post: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let latest_notice_post: Option<(NoticeId, ChainHash)> = LatestNotice::get(ChainId::Eth);
            let notice_hashes_post: Vec<(ChainHash, NoticeId)> = NoticeHashes::iter().collect();
            let account_notices_post: Vec<(ChainAccount, Vec<NoticeId>)> =
                AccountNotices::iter().collect();

            assert_eq!(
                asset_balances_pre - 50000000000000000000,
                asset_balances_post
            ); // 50e18
            assert_eq!(total_supply_pre - 50000000000000000000, total_supply_post);
            assert_eq!(total_borrows_pre, total_borrows_post);
            assert_eq!(events_pre.len() + 1, events_post.len());

            assert_eq!(notices_pre.len() + 1, notices_post.len());
            assert_eq!(notice_states_pre.len() + 1, notice_states_post.len());
            assert_ne!(latest_notice_pre, latest_notice_post);
            assert_eq!(notice_hashes_pre.len() + 1, notice_hashes_post.len());
            assert_eq!(account_notices_pre.len() + 1, account_notices_post.len());

            let notice_event = events_post.into_iter().next().unwrap();

            let notice = notices_post.into_iter().last().unwrap().1;
            let notice_state = notice_states_post.into_iter().last().unwrap();
            let latest_notice = latest_notice_post.unwrap();
            let notice_hash = notice_hashes_post.into_iter().last().unwrap();
            let account_notice = account_notices_post.into_iter().last().unwrap();

            let expected_notice_id = NoticeId(0, 1);
            let expected_notice = Notice::ExtractionNotice(ExtractionNotice::Eth {
                id: expected_notice_id,
                parent: [0u8; 32],
                asset: eth_asset,
                account: eth_recipient,
                amount: 50000000000000000000,
            });
            let expected_notice_encoded = expected_notice.encode_notice();
            let expected_notice_hash = expected_notice.hash();

            assert_eq!(notice, expected_notice.clone());
            assert_eq!(
                (
                    ChainId::Eth,
                    expected_notice_id,
                    NoticeState::Pending {
                        signature_pairs: ChainSignatureList::Eth(vec![])
                    }
                ),
                notice_state
            );
            assert_eq!((expected_notice_id, expected_notice_hash), latest_notice);
            assert_eq!((expected_notice_hash, expected_notice_id), notice_hash);
            assert_eq!((recipient, vec![expected_notice_id]), account_notice);

            assert_eq!(
                mock::Event::pallet_cash(crate::Event::Notice(
                    expected_notice_id,
                    expected_notice,
                    expected_notice_encoded
                )),
                notice_event.event
            );
        });
    }

    #[test]
    fn test_extract_internal_notice_ids() {
        let eth_asset = [238; 20];
        let asset = ChainAsset::Eth(eth_asset);
        let asset_info = AssetInfo {
            liquidity_factor: LiquidityFactor::from_nominal("1"),
            ..AssetInfo::minimal(asset, ETH)
        };
        let eth_holder = [0; 20];
        let eth_recipient = [0; 20];
        let holder = ChainAccount::Eth(eth_holder);
        let recipient = ChainAccount::Eth(eth_recipient);

        new_test_ext().execute_with(|| {
            SupportedAssets::insert(&asset, asset_info);
            Prices::insert(asset_info.ticker, 100_000); // $0.10
            let quantity = get_quantity::<Test>(asset, 50_000_000_000_000_000_000).unwrap();
            let hodl_balance = quantity.value * 5;
            AssetBalances::insert(asset, holder, hodl_balance as AssetBalance);
            AssetsWithNonZeroBalance::insert(holder, asset, ());
            TotalSupplyAssets::insert(&asset, hodl_balance);

            let notices_pre: Vec<(NoticeId, Notice)> = Notices::iter_prefix(ChainId::Eth).collect();
            let notice_states_pre: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let latest_notice_pre: Option<(NoticeId, ChainHash)> = LatestNotice::get(ChainId::Eth);
            let notice_hashes_pre: Vec<(ChainHash, NoticeId)> = NoticeHashes::iter().collect();
            let account_notices_pre: Vec<(ChainAccount, Vec<NoticeId>)> =
                AccountNotices::iter().collect();

            assert_eq!(LatestNotice::get(ChainId::Eth), None);
            assert_eq!(
                extract_internal::<Test>(asset_info, holder, recipient, quantity),
                Ok(())
            );

            let notice_state_post: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let notice_state = notice_state_post.into_iter().next().unwrap();
            let notice = Notices::get(notice_state.0, notice_state.1);

            let expected_notice_id = NoticeId(0, 1);
            let expected_notice = Notice::ExtractionNotice(ExtractionNotice::Eth {
                id: expected_notice_id,
                parent: [0u8; 32],
                asset: eth_asset,
                account: eth_recipient,
                amount: 50000000000000000000,
            });

            assert_eq!(
                (
                    ChainId::Eth,
                    expected_notice_id,
                    NoticeState::Pending {
                        signature_pairs: ChainSignatureList::Eth(vec![])
                    }
                ),
                notice_state
            );

            assert_eq!(notice, Some(expected_notice.clone()));

            assert_eq!(
                LatestNotice::get(ChainId::Eth),
                Some((NoticeId(0, 1), expected_notice.hash()))
            );
            assert_eq!(
                extract_internal::<Test>(asset_info, holder, recipient, quantity),
                Ok(())
            );

            let notices_post_2: Vec<(NoticeId, Notice)> =
                Notices::iter_prefix(ChainId::Eth).collect();
            let notice_states_post_2: Vec<(ChainId, NoticeId, NoticeState)> =
                NoticeStates::iter().collect();
            let latest_notice_post_2: Option<(NoticeId, ChainHash)> =
                LatestNotice::get(ChainId::Eth);
            let notice_hashes_post_2: Vec<(ChainHash, NoticeId)> = NoticeHashes::iter().collect();
            let account_notices_post_2: Vec<(ChainAccount, Vec<NoticeId>)> =
                AccountNotices::iter().collect();

            assert_eq!(notices_pre.len() + 2, notices_post_2.len());
            assert_eq!(notice_states_pre.len() + 2, notice_states_post_2.len());
            assert_ne!(latest_notice_pre, latest_notice_post_2);
            assert_eq!(notice_hashes_pre.len() + 2, notice_hashes_post_2.len());
            assert_eq!(account_notices_pre.len() + 1, account_notices_post_2.len());

            let latest_notice_2 = LatestNotice::get(ChainId::Eth).unwrap();
            let notice_2 = Notices::get(ChainId::Eth, latest_notice_2.0).unwrap();
            let notice_state_2 = NoticeStates::get(ChainId::Eth, latest_notice_2.0);
            let notice_hash_2 = NoticeHashes::get(latest_notice_2.1).unwrap();
            let account_notice_2 = AccountNotices::get(recipient);

            let expected_notice_2_id = NoticeId(0, 2);
            let expected_notice_2 = Notice::ExtractionNotice(ExtractionNotice::Eth {
                id: expected_notice_2_id,
                parent: <Ethereum as Chain>::hash_bytes(&expected_notice.encode_notice()),
                asset: eth_asset,
                account: eth_recipient,
                amount: 50000000000000000000,
            });
            let expected_notice_encoded_2 = expected_notice_2.encode_notice();
            let expected_notice_hash_2 = expected_notice_2.hash();

            assert_eq!(notice_2, expected_notice_2.clone());
            assert_eq!(
                NoticeState::Pending {
                    signature_pairs: ChainSignatureList::Eth(vec![])
                },
                notice_state_2
            );
            assert_eq!(
                (expected_notice_2_id, expected_notice_hash_2),
                latest_notice_2
            );
            assert_eq!(expected_notice_2_id, notice_hash_2);
            assert_eq!(
                vec![expected_notice_id, expected_notice_2_id],
                account_notice_2
            );
        });
    }

    #[test]
    fn test_compute_cash_principal_per() -> Result<(), Reason> {
        // round numbers (unrealistic but very easy to check)
        let asset_rate = APR::from_nominal("0.30"); // 30% per year
        let dt = MILLISECONDS_PER_YEAR / 2; // for 6 months
        let cash_index = CashIndex::from_nominal("1.5"); // current index value 1.5
        let price_asset = Price::from_nominal(CASH.ticker, "1500"); // $1,500
        let price_cash = Price::from_nominal(CASH.ticker, "1");
        let price_ratio = price_asset.ratio(price_cash)?;

        let actual = cash_index.cash_principal_per_asset(asset_rate.simple(dt)?, price_ratio)?;
        let expected = CashPrincipalPerAsset::from_nominal("150"); // from hand calc
        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn test_compute_cash_principal_per_specific_case() -> Result<(), Reason> {
        // a unit test related to previous unexpected larger scope test of on_initialize
        // this showed that we should divide by SECONDS_PER_YEAR last te prevent un-necessary truncation
        let asset_rate = APR::from_nominal("0.1225");
        let dt = MILLISECONDS_PER_YEAR / 4;
        let cash_index = CashIndex::from_nominal("1.123");
        let price_asset = Price::from_nominal(CASH.ticker, "1450");
        let price_cash = Price::from_nominal(CASH.ticker, "1");
        let price_ratio = price_asset.ratio(price_cash)?;

        let actual = cash_index.cash_principal_per_asset(asset_rate.simple(dt)?, price_ratio)?;
        let expected = CashPrincipalPerAsset::from_nominal("39.542520035618878005"); // from hand calc
        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn test_compute_cash_principal_per_realistic_underflow_case() -> Result<(), Reason> {
        // a unit test related to previous unexpected larger scope test of on_initialize
        // This case showed that we should have more decimals on CASH token to avoid 0 interest
        // showing for common cases. We want "number go up" technology.
        let asset_rate = APR::from_nominal("0.156");
        let dt = 6000;
        let cash_index = CashIndex::from_nominal("4.629065392511782467");
        let price_asset = Price::from_nominal(CASH.ticker, "0.313242");
        let price_cash = Price::from_nominal(CASH.ticker, "1");
        let price_ratio = price_asset.ratio(price_cash)?;

        let actual = cash_index.cash_principal_per_asset(asset_rate.simple(dt)?, price_ratio)?;
        let expected = CashPrincipalPerAsset::from_nominal("0.000000002008426366"); // from hand calc
        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn test_get_utilization() -> Result<(), Reason> {
        new_test_ext().execute_with(|| {
            initialize_storage();
            TotalSupplyAssets::insert(&Eth, 100);
            TotalBorrowAssets::insert(&Eth, 50);

            assert_eq!(
                crate::core::get_utilization::<Test>(Eth)?,
                Factor::from_nominal("0.5")
            );

            Ok(())
        })
    }

    #[test]
    fn test_get_borrow_rate() -> Result<(), Reason> {
        new_test_ext().execute_with(|| {
            initialize_storage();
            let kink_rate = 105;
            let asset = Eth;
            let asset_info = AssetInfo {
                rate_model: InterestRateModel::new_kink(
                    0,
                    kink_rate,
                    Factor::from_nominal("0.5"),
                    202,
                ),
                miner_shares: MinerShares::from_nominal("0.5"),
                ..AssetInfo::minimal(asset, ETH)
            };

            // 50% utilization and 50% miner shares
            SupportedAssets::insert(&asset, asset_info);
            TotalSupplyAssets::insert(&asset, 100);
            TotalBorrowAssets::insert(&asset, 50);

            assert_ok!(CashModule::set_rate_model(
                Origin::root(),
                asset,
                asset_info.rate_model
            ));

            let (borrow_rate, supply_rate) = crate::core::get_rates::<Test>(asset)?;
            assert_eq!(borrow_rate, kink_rate.into());
            assert_eq!(supply_rate, (kink_rate / 2 / 2).into());

            Ok(())
        })
    }

    #[test]
    fn test_has_liquidity_to_reduce_cash() -> Result<(), Reason> {
        const BAT: Units = Units::from_ticker_str("BAT", 18);
        let asset = ChainAsset::from_str("Eth:0x0d8775f648430679a709e98d2b0cb6250d2887ef")?;
        let asset_info = AssetInfo {
            liquidity_factor: LiquidityFactor::from_nominal("0.6543"),
            ..AssetInfo::minimal(asset, BAT)
        };

        new_test_ext().execute_with(|| {
            let account = ChainAccount::Eth([0u8; 20]);
            let amount = Quantity::from_nominal("5", CASH);

            assert!(!has_liquidity_to_reduce_cash::<Test>(account, amount)?);

            Prices::insert(BAT.ticker, Price::from_nominal(BAT.ticker, "0.53").value);
            SupportedAssets::insert(&asset, asset_info);
            AssetBalances::insert(&asset, &account, Balance::from_nominal("25000", BAT).value);
            AssetsWithNonZeroBalance::insert(account, asset, ());

            assert!(has_liquidity_to_reduce_cash::<Test>(account, amount)?);

            Ok(())
        })
    }

    #[test]
    fn test_on_initialize() {
        new_test_ext().execute_with(|| {
            // XXX how to inject miner?
            let miner = ChainAccount::Eth([0; 20]);
            let asset = Eth;
            let asset_info = AssetInfo {
                rate_model: InterestRateModel::new_kink(0, 2500, Factor::from_nominal("0.5"), 5000),
                miner_shares: MinerShares::from_nominal("0.02"),
                ..AssetInfo::minimal(asset, ETH)
            };
            // XXX how to inject now / last yield timestamp?
            let last_yield_timestamp = 10;
            let now = last_yield_timestamp + MILLISECONDS_PER_YEAR / 4; // 3 months go by

            SupportedAssets::insert(&asset, asset_info);
            GlobalCashIndex::put(CashIndex::from_nominal("1.123"));
            SupplyIndices::insert(&asset, AssetIndex::from_nominal("1234"));
            BorrowIndices::insert(&asset, AssetIndex::from_nominal("1345"));
            TotalSupplyAssets::insert(asset.clone(), asset_info.as_quantity_nominal("300").value);
            TotalBorrowAssets::insert(asset.clone(), asset_info.as_quantity_nominal("150").value);
            CashYield::put(APR::from_nominal("0.24")); // 24% APR big number for easy to see interest
            TotalCashPrincipal::put(CashPrincipalAmount::from_nominal("450000")); // 450k cash principal
            CashPrincipals::insert(&miner, CashPrincipal::from_nominal("1"));
            Prices::insert(asset_info.ticker, 1450_000000 as AssetPrice); // $1450 eth

            let result = on_initialize_internal::<Test>(now, last_yield_timestamp);
            assert_eq!(result, Ok(0u64));

            assert_eq!(
                SupplyIndices::get(&asset),
                AssetIndex::from_nominal("1273.542520035618878005")
            );
            assert_eq!(
                BorrowIndices::get(&asset),
                AssetIndex::from_nominal("1425.699020480854853072")
            );
            // note - the cash index number below is quite round due to the polynomial nature of
            // our approximation and the fact that the ratio in this case worked out to be a
            // base 10 number that terminates in that many digits.
            assert_eq!(
                GlobalCashIndex::get(),
                CashIndex::from_nominal("1.192441828000000000")
            );
            assert_eq!(
                TotalCashPrincipal::get(),
                CashPrincipalAmount::from_nominal("462104.853072128227960800")
            );
            assert_eq!(
                CashPrincipals::get(&miner),
                CashPrincipal::from_nominal("1.000000")
            );
            // Run again to set miner true principal
            assert_eq!(
                on_initialize_internal::<Test>(now, last_yield_timestamp),
                Ok(0)
            );
            assert_eq!(
                CashPrincipals::get(&miner),
                CashPrincipal::from_nominal("243.097061442564559300")
            );
        });
    }

    #[test]
    fn test_now() {
        new_test_ext().execute_with(|| {
            let expected = 123;
            <pallet_timestamp::Module<Test>>::set_timestamp(expected);
            let actual = get_now::<Test>();
            assert_eq!(actual, expected);
        });
    }

    #[test]
    fn test_set_asset_balance_internal() {
        new_test_ext().execute_with(|| {
            let asset1 = ChainAsset::Eth([1; 20]);
            let asset2 = ChainAsset::Eth([2; 20]);
            let asset3 = ChainAsset::Eth([3; 20]);
            let account = ChainAccount::Eth([20; 20]);

            let nonzero_balance = 1;
            let zero_balance = 0;
            // asset1 and asset2 both have nonzero balances
            AssetBalances::insert(asset1, account, nonzero_balance);
            AssetBalances::insert(asset2, account, nonzero_balance);
            AssetsWithNonZeroBalance::insert(account, asset1, ());
            AssetsWithNonZeroBalance::insert(account, asset2, ());

            set_asset_balance_internal::<Test>(asset1, account, zero_balance);
            assert!(
                !AssetsWithNonZeroBalance::contains_key(account, asset1),
                "set to zero should be zeroed out"
            );
            assert!(
                AssetsWithNonZeroBalance::contains_key(account, asset2),
                "should not be zeroed out"
            );
            assert_eq!(AssetBalances::get(asset1, account), zero_balance);
            assert_eq!(AssetBalances::get(asset2, account), nonzero_balance);

            set_asset_balance_internal::<Test>(asset3, account, nonzero_balance);
            assert!(
                !AssetsWithNonZeroBalance::contains_key(account, asset1),
                "set to zero should be zeroed out"
            );
            assert!(
                AssetsWithNonZeroBalance::contains_key(account, asset2),
                "should not be zeroed out"
            );
            assert!(
                AssetsWithNonZeroBalance::contains_key(account, asset3),
                "should not be zeroed out"
            );
            assert_eq!(AssetBalances::get(asset1, account), zero_balance);
            assert_eq!(AssetBalances::get(asset2, account), nonzero_balance);
            assert_eq!(AssetBalances::get(asset3, account), nonzero_balance);
        });
    }
}
