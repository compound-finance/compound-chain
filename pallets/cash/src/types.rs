use codec::{Decode, Encode};
use frame_support::sp_runtime::DispatchError;

use our_std::{
    collections::btree_set::BTreeSet,
    consts::{int_from_string_with_decimals, static_pow10, uint_from_string_with_decimals},
    convert::{TryFrom, TryInto},
    Deserialize, RuntimeDebug, Serialize,
};
use types_derive::{type_alias, Types};

pub use pallet_oracle::{ticker::Ticker, types::Price};

pub use crate::{
    chains::{Chain, ChainAccount, ChainAsset, ChainBlockNumber, ChainId, Ethereum},
    factor::{BigInt, BigUint, Factor},
    notices::{Notice, NoticeId},
    rates::{InterestRateModel, APR},
    reason::{MathError, Reason},
    symbol::{Symbol, Units, CASH, USD},
    SubstrateId,
};

// Type aliases //

/// Type for representing a percentage/fractional, often between [0, 100].
#[type_alias]
pub type Bips = u128;

/// Type for representing a number of decimal places.
#[type_alias]
pub type Decimals = u8;

/// Type for a nonce.
#[type_alias]
pub type Nonce = u32;

/// Type for representing time since current Unix epoch in milliseconds.
#[type_alias]
pub type Timestamp = timestamp::Timestamp; //u64;

/// Type of the largest possible signed integer.
#[type_alias]
pub type Int = i128;

/// Type of the largest possible unsigned integer.
#[type_alias]
pub type Uint = u128;

/// Type for a generic encoded message, potentially for any chain.
#[type_alias]
pub type EncodedNotice = Vec<u8>;

/// Type for representing an amount, potentially of any symbol.
#[type_alias]
pub type AssetAmount = Uint;

/// Type for representing an amount of CASH
#[type_alias]
pub type CashAmount = Uint;

/// Type for representing a balance of a specific asset.
#[type_alias]
pub type AssetBalance = Int;

/// Type for representing an amount of an asset, together with its units.
#[type_alias]
pub type AssetQuantity = Quantity;

/// Type for representing a quantity of CASH.
#[type_alias]
pub type CashQuantity = Quantity; // ideally Quantity<{ CASH }>

/// Type for representing a quantity of USD.
#[type_alias]
pub type USDQuantity = Quantity; // ideally Quantity<{ USD }>

/// Type for a market's liquidity factor.
#[type_alias]
pub type LiquidityFactor = Factor;

/// Type for the miner shares portion of interest going to miners.
#[type_alias]
pub type MinerShares = Factor;

/// Type for a code hash.
#[type_alias]
pub type CodeHash = <Ethereum as Chain>::Hash;

/// Governance Result type
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug, Types)]
pub enum GovernanceResult {
    FailedToDecodeCall,
    DispatchSuccess,
    DispatchFailure(DispatchError),
}

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
    pub eth_address: <Ethereum as Chain>::Address,
}

impl ValidatorKeys {
    pub fn miner_address(self) -> ChainAccount {
        ChainAccount::Gate(self.substrate_id.into())
    }
}

/// Type for referring to either an asset or CASH.
#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug, Types)]
pub enum CashOrChainAsset {
    Cash,
    ChainAsset(ChainAsset),
}

/// Type for representing a quantity, potentially of any symbol.
#[derive(Serialize, Deserialize)] // used in config
#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug, Types)]
pub struct AssetInfo {
    pub asset: ChainAsset,
    pub decimals: Decimals,
    pub liquidity_factor: LiquidityFactor,
    pub rate_model: InterestRateModel,
    pub miner_shares: MinerShares,
    pub supply_cap: AssetAmount,
    pub symbol: Symbol,
    pub ticker: Ticker,
}

impl AssetInfo {
    pub fn minimal(asset: ChainAsset, units: Units) -> Self {
        AssetInfo {
            asset,
            decimals: units.decimals,
            liquidity_factor: LiquidityFactor::default(),
            rate_model: InterestRateModel::default(),
            miner_shares: MinerShares::default(),
            supply_cap: AssetAmount::default(),
            symbol: Symbol(units.ticker.0),
            ticker: units.ticker,
        }
    }

    pub const fn units(self) -> Units {
        Units::new(self.ticker, self.decimals)
    }

    pub const fn as_balance(self, amount: AssetBalance) -> Balance {
        Balance::new(amount, self.units())
    }

    pub const fn as_balance_nominal(self, s: &'static str) -> Balance {
        Balance::from_nominal(s, self.units())
    }

    pub const fn as_quantity(self, amount: AssetAmount) -> Quantity {
        Quantity::new(amount, self.units())
    }

    pub const fn as_quantity_nominal(self, s: &'static str) -> Quantity {
        Quantity::from_nominal(s, self.units())
    }
}

// Note: ideally we would impl Ord ourselves for all these Ord types,
//  and assert ticker/units are the same when comparing.
// We would have to panic, though not for PartialOrd...

/// Type for representing a quantity of an asset, bound to its ticker and number of decimals.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug, Types)]
pub struct Quantity {
    pub value: AssetAmount,
    pub units: Units,
}

impl Quantity {
    pub const fn new(value: AssetAmount, units: Units) -> Self {
        Quantity { value, units }
    }

    /// Get a quantity from a string.
    /// Only for use in const contexts.
    pub const fn from_nominal(s: &'static str, units: Units) -> Self {
        Quantity::new(uint_from_string_with_decimals(units.decimals, s), units)
    }

    pub fn as_decrease(self) -> Result<Balance, MathError> {
        Ok(Balance::new(
            -self.value.try_into().map_err(|_| MathError::Overflow)?,
            self.units,
        ))
    }

    pub fn as_increase(self) -> Result<Balance, MathError> {
        Ok(Balance::new(
            self.value.try_into().map_err(|_| MathError::Overflow)?,
            self.units,
        ))
    }

    // Quantity<U> + Quantity<U> -> Quantity<U>
    pub fn add(self, rhs: Quantity) -> Result<Quantity, MathError> {
        if self.units != rhs.units {
            return Err(MathError::UnitsMismatch);
        }
        Ok(Quantity::new(
            self.value
                .checked_add(rhs.value)
                .ok_or(MathError::Overflow)?,
            self.units,
        ))
    }

    // Quantity<U> - Quantity<U> -> Quantity<U>
    pub fn sub(self, rhs: Quantity) -> Result<Quantity, MathError> {
        if self.units != rhs.units {
            return Err(MathError::UnitsMismatch);
        }
        Ok(Quantity::new(
            self.value
                .checked_sub(rhs.value)
                .ok_or(MathError::Underflow)?,
            self.units,
        ))
    }

    // Quantity<U.T> * Price<T> -> Quantity<{ USD }>
    pub fn mul_price(self, rhs: Price) -> Result<Quantity, MathError> {
        if self.units.ticker != rhs.ticker {
            return Err(MathError::UnitsMismatch);
        }
        let result = mul(
            self.value,
            self.units.decimals,
            rhs.value,
            Price::DECIMALS,
            USD.decimals,
        )?;
        Ok(Quantity::new(result as AssetAmount, USD))
    }

    // Quantity<{ USD }> / Price<T> -> Quantity<U.T>
    pub fn div_price(self, rhs: Price, units: Units) -> Result<Quantity, MathError> {
        if self.units != USD {
            return Err(MathError::PriceNotUSD);
        }
        if rhs.value == 0 {
            return Err(MathError::DivisionByZero);
        }
        if rhs.ticker != units.ticker {
            return Err(MathError::UnitsMismatch);
        }
        let result = div(
            self.value,
            self.units.decimals,
            rhs.value,
            Price::DECIMALS,
            units.decimals,
        )?;
        Ok(Quantity::new(result as AssetAmount, units))
    }

    // Quantity<U> * Factor -> Quantity<U>
    pub fn mul_factor(self, factor: Factor) -> Result<Quantity, MathError> {
        Ok(Quantity::new(
            BigUint::from_uint(self.value)
                .mul_decimal(factor.0, Factor::DECIMALS)
                .to_uint()?,
            self.units,
        ))
    }

    // Quantity<U> / Factor -> Quantity<U>
    pub fn div_factor(self, factor: Factor) -> Result<Quantity, MathError> {
        Ok(Quantity::new(
            BigUint::from_uint(self.value)
                .div_decimal(factor.0, Factor::DECIMALS)?
                .to_uint()?,
            self.units,
        ))
    }

    pub fn decay(self, nblocks: ChainBlockNumber) -> Result<Quantity, MathError> {
        // XXX TODO decide on what decay fn?
        //  currently V / 2^T
        let nbits = u32::try_from(nblocks).unwrap_or(u32::MAX);
        let value = self.value.checked_shr(nbits).unwrap_or(0);
        Ok(Quantity::new(value, self.units))
    }

    pub fn negate(self) -> Result<Balance, MathError> {
        let signed_value: i128 = self.value.try_into().map_err(|_| MathError::Overflow)?;
        let negative_value = signed_value.checked_neg().ok_or(MathError::Underflow)?;
        Ok(Balance {
            value: negative_value,
            units: self.units,
        })
    }
}

/// Type for representing a signed balance of an asset, bound to its ticker and number of decimals.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug, Types)]
pub struct Balance {
    pub value: AssetBalance,
    pub units: Units,
}

impl Balance {
    pub const fn new(value: AssetBalance, units: Units) -> Self {
        Balance { value, units }
    }

    /// Get a quantity from a string.
    /// Only for use in const contexts.
    pub const fn from_nominal(s: &'static str, units: Units) -> Self {
        Balance::new(int_from_string_with_decimals(units.decimals, s), units)
    }

    // Balance<U> + Balance<U> -> Balance<U>
    pub fn add(self, delta: Balance) -> Result<Balance, MathError> {
        if self.units.ticker != delta.units.ticker {
            return Err(MathError::UnitsMismatch);
        }
        Ok(Balance::new(
            self.value
                .checked_add(delta.value)
                .ok_or(MathError::Overflow)?,
            self.units,
        ))
    }

    // Balance<U> + Quantity<U> -> Balance<U>
    pub fn add_quantity(self, delta: Quantity) -> Result<Balance, MathError> {
        if self.units.ticker != delta.units.ticker {
            return Err(MathError::UnitsMismatch);
        }
        Ok(Balance::new(
            self.value
                .checked_add(delta.value.try_into().map_err(|_| MathError::Overflow)?)
                .ok_or(MathError::Underflow)?,
            self.units,
        ))
    }

    // Balance<U> - Quantity<U> -> Balance<U>
    pub fn sub_quantity(self, delta: Quantity) -> Result<Balance, MathError> {
        if self.units.ticker != delta.units.ticker {
            return Err(MathError::UnitsMismatch);
        }
        Ok(Balance::new(
            self.value
                .checked_sub(delta.value.try_into().map_err(|_| MathError::Overflow)?)
                .ok_or(MathError::Underflow)?,
            self.units,
        ))
    }

    // Balance<U.T> * Price<T> -> Balance<{ USD }>
    pub fn mul_price(self, rhs: Price) -> Result<Balance, MathError> {
        if self.units.ticker != rhs.ticker {
            return Err(MathError::UnitsMismatch);
        }
        let result = mul_int(
            self.value,
            self.units.decimals,
            rhs.value.try_into().map_err(|_| MathError::Overflow)?,
            Price::DECIMALS,
            USD.decimals,
        )?;
        Ok(Balance::new(result, USD))
    }

    // Balance<U> / LiquidityFactor -> Balance<U>
    pub fn div_factor(self, rhs: LiquidityFactor) -> Result<Balance, MathError> {
        let result = div_int(
            self.value,
            self.units.decimals,
            rhs.0.try_into().map_err(|_| MathError::Overflow)?,
            LiquidityFactor::DECIMALS,
            self.units.decimals,
        )?;
        Ok(Balance::new(result, self.units))
    }

    // Balance<U> * LiquidityFactor -> Balance<U>
    pub fn mul_factor(self, rhs: LiquidityFactor) -> Result<Balance, MathError> {
        let result = mul_int(
            self.value,
            self.units.decimals,
            rhs.0.try_into().map_err(|_| MathError::Overflow)?,
            LiquidityFactor::DECIMALS,
            self.units.decimals,
        )?;
        Ok(Balance::new(result, self.units))
    }

    /// Returns true if the balance is greater than or equal to given value
    pub fn gte(self: &Self, v: i128) -> bool {
        self.value >= v
    }

    /// Returns true if the balance is less than or equal to given value
    pub fn lte(self: &Self, v: i128) -> bool {
        self.value <= v
    }
}

impl TryInto<Balance> for Quantity {
    type Error = Reason;

    fn try_into(self) -> Result<Balance, Reason> {
        match i128::try_from(self.value) {
            Ok(v) => Ok(Balance {
                value: v,
                units: self.units,
            }),
            Err(_) => Err(Reason::MathError(MathError::Overflow)),
        }
    }
}

/// Type for representing a balance of CASH Principal.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, Default, RuntimeDebug, Types,
)]
pub struct CashPrincipal(pub AssetBalance);

impl CashPrincipal {
    pub const DECIMALS: Decimals = CASH.decimals;
    pub const ZERO: CashPrincipal = CashPrincipal(0);
    pub const ONE: CashPrincipal = CashPrincipal::from_nominal("1");

    /// Get a CASH principal balance from a string.
    /// Only for use in const contexts.
    pub const fn from_nominal(s: &'static str) -> Self {
        CashPrincipal(int_from_string_with_decimals(Self::DECIMALS, s))
    }

    pub fn add(self: Self, rhs: Self) -> Result<Self, MathError> {
        Ok(CashPrincipal(
            self.0.checked_add(rhs.0).ok_or(MathError::Overflow)?,
        ))
    }

    pub fn sub(self, rhs: Self) -> Result<Self, MathError> {
        Ok(CashPrincipal(
            self.0.checked_sub(rhs.0).ok_or(MathError::Underflow)?,
        ))
    }

    pub fn add_amount(self, amount: CashPrincipalAmount) -> Result<Self, MathError> {
        let signed = AssetBalance::try_from(amount.0).or(Err(MathError::Overflow))?;
        self.add(CashPrincipal(signed))
    }

    pub fn sub_amount(self, amount: CashPrincipalAmount) -> Result<Self, MathError> {
        let signed = AssetBalance::try_from(amount.0).or(Err(MathError::Overflow))?;
        self.sub(CashPrincipal(signed))
    }

    pub fn amount_withdrawable(self) -> Result<CashPrincipalAmount, MathError> {
        if self.0 > 0 {
            Ok(CashPrincipalAmount(self.0 as AssetAmount))
        } else {
            Ok(CashPrincipalAmount(0))
        }
    }

    pub fn amount_repayable(self) -> Result<CashPrincipalAmount, MathError> {
        if self.0 < 0 {
            Ok(CashPrincipalAmount(-self.0 as AssetAmount))
        } else {
            Ok(CashPrincipalAmount(0))
        }
    }

    pub fn negate(self) -> Self {
        Self(-self.0)
    }

    /// Returns true if the principal is equals value
    pub fn eq(self: &Self, v: i128) -> bool {
        self.0 == v
    }

    /// Returns true if the principal is greater than or equal to given value
    pub fn gte(self: &Self, v: i128) -> bool {
        self.0 >= v
    }

    /// Returns true if the principal is less than or equal to given value
    pub fn lte(self: &Self, v: i128) -> bool {
        self.0 <= v
    }
}

/// Type for representing an amount of CASH Principal.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, Default, RuntimeDebug, Types,
)]
pub struct CashPrincipalAmount(pub AssetAmount);

impl CashPrincipalAmount {
    pub const DECIMALS: Decimals = CashPrincipal::DECIMALS;
    pub const ZERO: CashPrincipalAmount = CashPrincipalAmount(0);

    /// Get a CASH principal amount from a string.
    /// Only for use in const contexts.
    pub const fn from_nominal(s: &'static str) -> Self {
        CashPrincipalAmount(uint_from_string_with_decimals(Self::DECIMALS, s))
    }

    pub fn add(self: Self, rhs: Self) -> Result<Self, MathError> {
        Ok(CashPrincipalAmount(
            self.0.checked_add(rhs.0).ok_or(MathError::Overflow)?,
        ))
    }

    pub fn sub(self, rhs: Self) -> Result<Self, MathError> {
        Ok(CashPrincipalAmount(
            self.0.checked_sub(rhs.0).ok_or(MathError::Underflow)?,
        ))
    }
}

impl TryInto<CashPrincipal> for CashPrincipalAmount {
    type Error = Reason;

    fn try_into(self) -> Result<CashPrincipal, Reason> {
        match i128::try_from(self.0) {
            Ok(v) => Ok(CashPrincipal(v)),
            Err(_) => Err(Reason::MathError(MathError::SignMismatch)),
        }
    }
}

impl TryInto<CashPrincipalAmount> for CashPrincipal {
    type Error = Reason;

    fn try_into(self) -> Result<CashPrincipalAmount, Reason> {
        match u128::try_from(self.0) {
            Ok(v) => Ok(CashPrincipalAmount(v)),
            Err(_) => Err(Reason::MathError(MathError::SignMismatch)),
        }
    }
}

/// Type for representing a multiplicative index on Gateway.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug, Types)]
pub struct CashIndex(pub Uint);

#[type_alias]
pub type CashPerCashPrincipal = CashIndex;

impl CashIndex {
    pub const DECIMALS: Decimals = Factor::DECIMALS;
    pub const ONE: CashIndex = CashIndex(static_pow10(Self::DECIMALS));

    /// Get a CASH index from a string.
    /// Only for use in const contexts.
    pub const fn from_nominal(s: &'static str) -> Self {
        CashIndex(uint_from_string_with_decimals(Self::DECIMALS, s))
    }

    // CashPrincipal * CashIndex -> Balance<{ CASH }>
    pub fn cash_balance(self, principal: CashPrincipal) -> Result<Balance, MathError> {
        Ok(Balance::new(
            BigInt::from_uint(self.0)
                .mul_decimal(principal.0, CashPrincipal::DECIMALS)
                .convert(Self::DECIMALS, CASH.decimals)
                .to_int()?,
            CASH,
        ))
    }

    // CashPrincipal(+) * CashIndex -> Quantity<{ CASH }>
    pub fn cash_quantity(
        self,
        principal_amount: CashPrincipalAmount,
    ) -> Result<Quantity, MathError> {
        Ok(Quantity::new(
            BigUint::from_uint(self.0)
                .mul_decimal(principal_amount.0, CashPrincipal::DECIMALS)
                .convert(Self::DECIMALS, CASH.decimals)
                .to_uint()?,
            CASH,
        ))
    }

    // Quantity<{ CASH }> / CashIndex -> CashPrincipal(+)
    pub fn cash_principal_amount(
        self,
        quantity: Quantity,
    ) -> Result<CashPrincipalAmount, MathError> {
        Ok(CashPrincipalAmount(
            BigUint::from_uint(Self::ONE.0)
                .mul_decimal(quantity.value, quantity.units.decimals)
                .div_decimal(self.0, Self::DECIMALS)?
                .convert(Self::DECIMALS, CashPrincipalAmount::DECIMALS)
                .to_uint()?,
        ))
    }

    /// Compute the amount of CASH principal per unit of asset,
    ///  given the rate, CASH index, prices, and length of time (ms).
    // Factor(rate*dt) * Factor(price_asset/price_cash) / CashIndex -> AssetIndex
    pub fn cash_principal_per_asset(
        self,
        multiplier: Factor,
        cash_per_asset: Factor,
    ) -> Result<CashPrincipalPerAsset, Reason> {
        Ok(AssetIndex(
            multiplier
                .mul(cash_per_asset)
                .div_decimal(self.0, Self::DECIMALS)?
                .convert(Factor::DECIMALS, CashPrincipalPerAsset::DECIMALS)
                .to_uint()?,
        ))
    }

    /// Multiply the index by the increase in cash per cash principal.
    pub fn increment(self, rhs: CashPerCashPrincipal) -> Result<Self, MathError> {
        Ok(CashIndex(
            BigUint::from_uint(self.0)
                .mul_decimal(rhs.0, CashPerCashPrincipal::DECIMALS)
                .to_uint()?,
        ))
    }
}

impl Default for CashIndex {
    fn default() -> Self {
        CashIndex::ONE
    }
}

impl From<Factor> for CashIndex {
    fn from(factor: Factor) -> Self {
        CashIndex(factor.0)
    }
}

impl<T> From<T> for CashIndex
where
    T: Into<Uint>,
{
    fn from(raw: T) -> Self {
        CashIndex(raw.into())
    }
}

/// Type for representing the additive asset indices.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug, Types)]
pub struct AssetIndex(pub Uint);

#[type_alias]
pub type CashPrincipalPerAsset = AssetIndex;

impl AssetIndex {
    pub const DECIMALS: Decimals = Factor::DECIMALS;
    pub const ONE: AssetIndex = AssetIndex::from_nominal("1");

    /// Get an asset index from a string.
    /// Only for use in const contexts.
    pub const fn from_nominal(s: &'static str) -> Self {
        AssetIndex(uint_from_string_with_decimals(Self::DECIMALS, s))
    }

    /// Get the change in CASH principal generated by a balance, since a previous index.
    pub fn cash_principal_since(
        self,
        since: AssetIndex,
        balance: Balance,
    ) -> Result<CashPrincipal, MathError> {
        let delta_index = self.0.checked_sub(since.0).ok_or(MathError::Underflow)?;
        Ok(CashPrincipal(
            BigInt::from_uint(delta_index)
                .mul_decimal(balance.value, balance.units.decimals)
                .convert(AssetIndex::DECIMALS, CashPrincipal::DECIMALS)
                .to_int()?,
        ))
    }

    // AssetQuantity<U> * CashPrincipalPerAsset(+) -> CashPrincipal(+)
    pub fn cash_principal_amount(
        self,
        amount: AssetQuantity,
    ) -> Result<CashPrincipalAmount, MathError> {
        Ok(CashPrincipalAmount(
            BigUint::from_uint(self.0)
                .mul_decimal(amount.value, amount.units.decimals)
                .convert(AssetIndex::DECIMALS, CashPrincipalAmount::DECIMALS)
                .to_uint()?,
        ))
    }

    /// Add an amount of cash principal per unit of asset to the index.
    pub fn increment(self, rhs: CashPrincipalPerAsset) -> Result<Self, MathError> {
        Ok(AssetIndex(
            self.0.checked_add(rhs.0).ok_or(MathError::Overflow)?,
        ))
    }
}

impl Default for AssetIndex {
    fn default() -> Self {
        AssetIndex(0)
    }
}

/// Multiply floating point numbers represented by a (value, number_of_decimals) pair and specify
/// the output number of decimals.
///
/// Not recommended to use directly, to be used in SafeMath implementations.
pub fn mul(
    a: Uint,
    a_decimals: Decimals,
    b: Uint,
    b_decimals: Decimals,
    out_decimals: Decimals,
) -> Result<Uint, MathError> {
    let all_numerator_decimals = a_decimals
        .checked_add(b_decimals)
        .ok_or(MathError::Overflow)?;
    if all_numerator_decimals > out_decimals {
        // scale down
        let scale_decimals = all_numerator_decimals
            .checked_sub(out_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10u128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_mul(b)
            .ok_or(MathError::Overflow)?
            .checked_div(scale)
            .ok_or(MathError::DivisionByZero)?)
    } else {
        // scale up
        let scale_decimals = out_decimals
            .checked_sub(all_numerator_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10u128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_mul(b)
            .ok_or(MathError::Overflow)?
            .checked_mul(scale)
            .ok_or(MathError::Overflow)?)
    }
}

/// Multiply floating point numbers represented by a (value, number_of_decimals) pair and specify
/// the output number of decimals.
///
/// Not recommended to use directly, to be used in SafeMath implementations.
pub fn mul_int(
    a: Int,
    a_decimals: u8,
    b: Int,
    b_decimals: u8,
    out_decimals: u8,
) -> Result<Int, MathError> {
    let all_numerator_decimals = a_decimals
        .checked_add(b_decimals)
        .ok_or(MathError::Overflow)?;
    if all_numerator_decimals > out_decimals {
        // scale down
        let scale_decimals = all_numerator_decimals
            .checked_sub(out_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10i128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_mul(b)
            .ok_or(MathError::Overflow)?
            .checked_div(scale)
            .ok_or(MathError::DivisionByZero)?)
    } else {
        // scale up
        let scale_decimals = out_decimals
            .checked_sub(all_numerator_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10i128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_mul(b)
            .ok_or(MathError::Overflow)?
            .checked_mul(scale)
            .ok_or(MathError::Overflow)?)
    }
}

/// Divide floating point numbers represented by a (value, number_of_decimals) pair and specify
/// the output number of decimals.
///
/// Not recommended to use directly, to be used in SafeMath implementations.
pub fn div(
    a: Uint,
    a_decimals: Decimals,
    b: Uint,
    b_decimals: Decimals,
    out_decimals: Decimals,
) -> Result<Uint, MathError> {
    let denom_decimals = b_decimals
        .checked_add(out_decimals)
        .ok_or(MathError::Overflow)?;
    if denom_decimals > a_decimals {
        // scale up
        let scale_decimals = denom_decimals
            .checked_sub(a_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10u128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_mul(scale)
            .ok_or(MathError::Overflow)?
            .checked_div(b)
            .ok_or(MathError::DivisionByZero)?)
    } else {
        // scale down
        let scale_decimals = a_decimals
            .checked_sub(denom_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10u128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_div(b)
            .ok_or(MathError::DivisionByZero)?
            .checked_div(scale)
            .ok_or(MathError::DivisionByZero)?)
    }
}

/// Divide floating point numbers represented by a (value, number_of_decimals) pair and specify
/// the output number of decimals.
///
/// Not recommended to use directly, to be used in SafeMath implementations.
pub fn div_int(
    a: Int,
    a_decimals: u8,
    b: Int,
    b_decimals: u8,
    out_decimals: u8,
) -> Result<Int, MathError> {
    let denom_decimals = b_decimals
        .checked_add(out_decimals)
        .ok_or(MathError::Overflow)?;
    if denom_decimals > a_decimals {
        // scale up
        let scale_decimals = denom_decimals
            .checked_sub(a_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10i128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_mul(scale)
            .ok_or(MathError::Overflow)?
            .checked_div(b)
            .ok_or(MathError::DivisionByZero)?)
    } else {
        // scale down
        let scale_decimals = a_decimals
            .checked_sub(denom_decimals)
            .ok_or(MathError::Underflow)?;
        let scale = 10i128
            .checked_pow(scale_decimals as u32)
            .ok_or(MathError::Overflow)?;
        Ok(a.checked_div(b)
            .ok_or(MathError::DivisionByZero)?
            .checked_div(scale)
            .ok_or(MathError::DivisionByZero)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::*;

    const ETH: Units = Units::from_ticker_str("ETH", 18);

    #[test]
    fn test_scale_codec() {
        let a = Quantity::from_nominal("3", CASH);
        let encoded = a.encode();
        let decoded = Decode::decode(&mut encoded.as_slice());
        let b = decoded.expect("value did not decode");
        assert_eq!(a, b);
    }

    #[test]
    fn test_from_nominal_with_all_decimals() {
        let a = Quantity::from_nominal("123.456789", CASH);
        let b = Quantity::new(123456789, CASH);
        assert_eq!(a, b);
    }

    #[test]
    fn test_from_nominal_with_less_than_all_decimals() {
        let a = Quantity::from_nominal("123.4", CASH);
        let b = Quantity::new(CASH.one() * 1234 / 10, CASH);
        assert_eq!(a, b);
    }

    #[test]
    fn test_from_nominal_with_no_decimals() {
        let a = Quantity::from_nominal("123", CASH);
        let b = Quantity::new(CASH.one() * 123, CASH);
        assert_eq!(a, b);
    }

    #[test]
    #[should_panic]
    fn test_from_nominal_input_string_value_out_of_range_high() {
        Quantity::from_nominal(":", CASH);
    }

    #[test]
    #[should_panic]
    fn test_from_nominal_input_string_value_out_of_range_low() {
        Quantity::from_nominal("/", CASH);
    }

    #[test]
    #[should_panic]
    fn test_from_nominal_multiple_radix() {
        Quantity::from_nominal("12.34.56", CASH);
    }

    #[test]
    #[should_panic]
    fn test_from_nominal_only_radix() {
        Quantity::from_nominal(".", CASH);
    }

    #[test]
    #[should_panic]
    fn test_from_nominal_only_radix_multiple() {
        Quantity::from_nominal("...", CASH);
    }

    #[test]
    fn test_mul_with_scale_output_equal() {
        let result = mul(2000, 3, 30000, 4, 7);
        assert_eq!(result, Ok(60000000));
    }

    #[test]
    fn test_mul_with_scale_output_up() {
        let result = mul(2000, 3, 30000, 4, 8);
        assert_eq!(result, Ok(600000000));
    }

    #[test]
    fn test_mul_with_scale_output_down() {
        let result = mul(2000, 3, 30000, 4, 6);
        assert_eq!(result, Ok(6000000));
    }

    #[test]
    fn test_div_with_scale_output_equal() {
        let result = div(2000, 3, 30000, 4, 7);
        assert_eq!(result, Ok(6666666));
    }

    #[test]
    fn test_div_with_scale_output_up() {
        let result = div(2000, 3, 30000, 4, 8);
        assert_eq!(result, Ok(66666666));
    }

    #[test]
    fn test_div_with_scale_output_down() {
        let result = div(2000, 3, 30000, 4, 6);
        assert_eq!(result, Ok(666666));
    }

    #[test]
    fn test_add_quantities() {
        let a = Quantity::from_nominal("5.5", ETH);
        let b = Quantity::from_nominal("6.5", ETH);
        assert_eq!(a.add(b), Ok(Quantity::from_nominal("12", ETH)));
    }

    #[test]
    fn test_sub_quantities() {
        let a = Quantity::from_nominal("5.5", ETH);
        let b = Quantity::from_nominal("6.5", ETH);
        assert_eq!(b.sub(a), Ok(Quantity::from_nominal("1", ETH)));
        assert_eq!(a.sub(b), Err(MathError::Underflow));
    }

    #[test]
    fn test_quantity_times_price() {
        let price = Price::from_nominal(ETH.ticker, "1500");
        let quantity = Quantity::from_nominal("5.5", ETH);
        let result = quantity.mul_price(price).unwrap();
        let expected = Quantity::from_nominal("8250", USD);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_quantity_over_price() {
        // same as example above just inverted
        let price = Price::from_nominal(ETH.ticker, "1500");
        let value = Quantity::from_nominal("8250", USD);
        let number_of_eth = value.div_price(price, ETH).unwrap();
        let expected_number_of_eth = Quantity::from_nominal("5.5", ETH);
        assert_eq!(number_of_eth, expected_number_of_eth);
    }

    #[test]
    fn test_quantity_decay() {
        let value = Quantity::from_nominal("8250", USD);
        assert_eq!(value.decay(1), Ok(Quantity::from_nominal("4125", USD)));
        assert_eq!(value.decay(2), Ok(Quantity::from_nominal("2062.5", USD)));
        assert_eq!(value.decay(3), Ok(Quantity::from_nominal("1031.25", USD)));
        assert_eq!(value.decay(4), Ok(Quantity::from_nominal("515.625", USD)));
        assert_eq!(value.decay(5), Ok(Quantity::from_nominal("257.8125", USD)));
        assert_eq!(value.decay(6), Ok(Quantity::from_nominal("128.90625", USD)));
        assert_eq!(value.decay(7), Ok(Quantity::from_nominal("64.453125", USD)));
        assert_eq!(value.decay(14), Ok(Quantity::new(503540, USD)));
        assert_eq!(value.decay(28), Ok(Quantity::new(30, USD)));
        assert_eq!(value.decay(35), Ok(Quantity::new(0, USD)));
        assert_eq!(value.decay(1000000), Ok(Quantity::new(0, USD)));
    }

    #[test]
    fn test_mul_index() {
        let index = CashIndex::from_nominal("1.01");
        let principal = CashPrincipalAmount::from_nominal("100");
        let quantity = index.cash_quantity(principal).unwrap();
        let principal_ = index.cash_principal_amount(quantity).unwrap();
        assert_eq!(quantity, Quantity::from_nominal("101", CASH));
        assert_eq!(principal_, principal);
    }

    #[test]
    fn test_mul_overflow() {
        let result = mul(Uint::max_value() / 2 + 1, 0, 2, 0, 0);
        assert_eq!(result, Err(MathError::Overflow));
    }

    #[test]
    fn test_mul_overflow_boundary() {
        let result = mul(Uint::max_value(), 0, 1, 0, 0);
        assert_eq!(result, Ok(Uint::max_value()));
    }

    #[test]
    fn test_mul_overflow_boundary_2() {
        // note max value is odd thus truncated here and we lose a digit
        let result = mul(Uint::max_value() / 2, 0, 2, 0, 0);
        assert_eq!(result, Ok(Uint::max_value() - 1));
    }

    #[test]
    fn test_div_by_zero() {
        let result = div(1, 0, 0, 0, 0);
        assert_eq!(result, Err(MathError::DivisionByZero));
    }

    #[test]
    fn test_div_overflow_decimals() {
        let result = div(1, 0, 1, 0, Decimals::max_value());
        assert_eq!(result, Err(MathError::Overflow));
    }

    #[test]
    fn test_div_overflow_decimals_2() {
        let result = div(1, Decimals::max_value(), 1, 0, 0);
        assert_eq!(result, Err(MathError::Overflow));
    }

    #[test]
    fn test_cash_index_increment() {
        let old_index = CashIndex::from_nominal("1.1"); // current 10%
        let increment = CashIndex::from_nominal("1.01"); // increment by 1%
        let actual = old_index.increment(increment).unwrap();
        let expected = CashIndex::from_nominal("1.1110");
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_int_from_string_with_decimals() {
        let x = int_from_string_with_decimals(3, "-12.345");
        assert_eq!(x, -12345);
    }

    #[test]
    fn test_cash_principal_since() {
        let old_index = AssetIndex::from_nominal("1.0");
        let cur_index = AssetIndex::from_nominal("1.1");
        let balance = Balance::from_nominal("100", ETH);
        let actual = cur_index.cash_principal_since(old_index, balance).unwrap();
        let expected = CashPrincipal::from_nominal("10");
        assert_eq!(actual, expected);
    }
}
