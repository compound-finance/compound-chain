use crate::{
    chains::{Chain, ChainId, Ethereum},
    notices::Notice,
    symbol::{Symbol, CASH, NIL, USD},
    Config, Module,
};
use codec::{Decode, Encode};
use num_traits::Pow;
use our_std::{
    ops::{Div, Mul},
    RuntimeDebug,
};

// Type aliases //

/// Type for representing an annualized rate on Compound Chain.
pub type APR = u128; // XXX custom rate type?

/// Type for a nonce.
pub type Nonce = u32;

/// Type for representing time on Compound Chain.
pub type Timestamp = u128; // XXX u64?

/// Type of the largest possible unsigned integer on Compound Chain.
pub type Uint = u128;

/// Type for a generic encoded message, potentially for any chain.
pub type EncodedNotice = Vec<u8>;

/// Type for representing a price, potentially for any symbol.
pub type AssetPrice = Uint;

/// Type for representing an amount of any asset
pub type AssetAmount = Uint;

/// Type for representing an amount of Cash
pub type CashAmount = Uint;

/// Type for representing a quantity, potentially of any symbol.
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum Maxable<T> {
    Max,
    Value(T),
}

impl<T> From<T> for Maxable<T> {
    fn from(t: T) -> Self {
        Maxable::Value(t)
    }
}

pub fn get_max_value<T>(max: Maxable<T>, when_max: &dyn Fn() -> T) -> T {
    match max {
        Maxable::Max => when_max(),
        Maxable::Value(t) => t,
    }
}

/// Either an AssetAmount or max
pub type MaxableAssetAmount = Maxable<AssetAmount>;

/// Type for chain signatures
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum ChainSignature {
    Eth(<Ethereum as Chain>::Signature),
}

/// Type for a list of chain signatures
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum ChainSignatureList {
    Eth(Vec<<Ethereum as Chain>::Signature>),
}

/// Type for chain accounts
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum ChainAccount {
    Eth(<Ethereum as Chain>::Address),
}

/// Type for chain assets
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum ChainAsset {
    Eth(<Ethereum as Chain>::Address),
}

/// Type for a set of open price feed reporters.
pub type ReporterSet = Vec<<Ethereum as Chain>::Address>;

/// Type for an encoded payload within an extrinsic.
pub type SignedPayload = Vec<u8>; // XXX

/// Type for signature used to verify that a signed payload comes from a validator.
pub type ValidatorSig = [u8; 65]; // XXX secp256k1 sign, but why secp256k1?

/// Type for an address used to identify a validator.
pub type ValidatorKey = [u8; 20]; // XXX secp256k1 public key, but why secp256k1?

/// Type for a set of validator identities.
pub type ValidatorSet = Vec<ValidatorKey>; // XXX whats our set type? ordered Vec?

/// Type for a set of configuration values
pub type ConfigSet<T> = Vec<T>;

/// Type for a set of configuration strings
pub type ConfigSetString = ConfigSet<String>;

/// Type for the status of an event on the queue.
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum EventStatus<C: Chain> {
    Pending {
        signers: crate::ValidatorSet,
    },
    Failed {
        hash: C::Hash,
        reason: crate::Reason,
    },
    Done,
}

/// Type for the status of a notice on the queue.
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum NoticeStatus {
    Missing,
    Pending {
        signers: crate::ValidatorSet,
        signatures: ChainSignatureList,
        notice: Notice,
    },
    Done,
}

/// Type for representing a price (in USD), bound to its symbol.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct Price(pub Symbol, pub AssetPrice);

/// Type for representing a quantity of an asset, bound to its symbol.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct Quantity(pub Symbol, pub AssetAmount);

/// Type for representing a multiplicative index on Compound Chain.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct MulIndex(pub Uint);

impl MulIndex {
    pub const DECIMALS: u8 = 4;

    pub const ONE: MulIndex = MulIndex(10000);
}

impl Default for MulIndex {
    fn default() -> Self {
        MulIndex(1)
    }
}

impl<T> From<T> for MulIndex
where
    T: Into<Uint>,
{
    fn from(raw: T) -> Self {
        MulIndex(raw.into())
    }
}

impl Price {
    pub const DECIMALS: u8 = USD.decimals(); // Note: must be >= USD.decimals()

    pub const fn symbol(&self) -> Symbol {
        self.0
    }

    pub const fn amount(&self) -> AssetPrice {
        self.1
    }
}

impl Quantity {
    pub const fn symbol(&self) -> Symbol {
        self.0
    }

    pub const fn amount(&self) -> AssetAmount {
        self.1
    }
}

#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub enum MathError {
    Overflowed,
    SymbolMismatch,
    PriceNotUSD,
    DivisionByZero,
}

pub trait SafeMul<Rhs = Self> {
    /// The resulting type after multiplying
    type Output;

    /// multiply self by the "right hand side" (RHS)
    fn safe_mul(self, rhs: Rhs) -> Result<Self::Output, MathError>;
}

// Price<S> * Quantity<S> -> Quantity<{ USD }>
impl SafeMul<Quantity> for Price {
    type Output = Quantity;

    fn safe_mul(self, rhs: Quantity) -> Result<Self::Output, MathError> {
        if self.symbol() != rhs.symbol() {
            return Err(MathError::SymbolMismatch);
        }

        let (numerator, overflowed) = self.amount().overflowing_mul(rhs.amount());
        if overflowed {
            return Err(MathError::Overflowed);
        }
        let (decimals, overflowed) = Price::DECIMALS.overflowing_add(rhs.symbol().decimals());
        if overflowed {
            return Err(MathError::Overflowed);
        }

        let (decimals, overflowed) = decimals.overflowing_sub(USD.decimals());
        if overflowed {
            return Err(MathError::Overflowed);
        }

        let (divisor, overflowed) = 10u128.overflowing_pow(decimals as u32);
        if overflowed {
            // this should almost never happen. it would have to be quite a large number of decimals
            return Err(MathError::Overflowed);
        }

        let (result, overflowed) = numerator.overflowing_div(divisor);
        if overflowed {
            return Err(MathError::Overflowed);
        }

        Ok(Quantity(USD, result as AssetAmount))
    }
}

// Quantity<S> * Price<S> -> Quantity<{ USD }>
impl SafeMul<Price> for Quantity {
    type Output = Quantity;

    fn safe_mul(self, rhs: Price) -> Result<Self::Output, MathError> {
        // in this case, multiplication is transitive (unlike matrix multiplication for example)
        rhs.safe_mul(self)
    }
}

pub trait SafeDiv<Rhs = Self> {
    /// The resulting type after dividing
    type Output;

    /// divide self by the "right hand side" (RHS)
    fn safe_div(self, rhs: Rhs) -> Result<Self::Output, MathError>;
}

// Quantity<{ USD }> / Price<S> -> Quantity<S>
impl SafeDiv<Price> for Quantity {
    type Output = Quantity;

    fn safe_div(self, rhs: Price) -> Result<Self::Output, MathError> {
        if self.symbol() != USD {
            return Err(MathError::PriceNotUSD);
        }
        if rhs.amount() == 0 {
            return Err(MathError::DivisionByZero);
        }
        let (scalar, overflowed) = 10u128.overflowing_pow(rhs.symbol().decimals() as u32);
        if overflowed {
            return Err(MathError::Overflowed);
        }

        let (numerator, overflowed) = scalar.overflowing_mul(self.amount());
        if overflowed {
            return Err(MathError::Overflowed);
        }

        let (result, overflowed) = numerator.overflowing_div(rhs.amount());
        if overflowed {
            return Err(MathError::Overflowed);
        }

        Ok(Quantity(rhs.symbol(), result as AssetAmount))
    }
}

// Quantity<S> * MulIndex -> Quantity<S>
impl SafeMul<MulIndex> for Quantity {
    type Output = Quantity;

    fn safe_mul(self, rhs: MulIndex) -> Result<Self::Output, MathError> {
        let (numerator, overflowed) = rhs.0.overflowing_mul(self.amount());
        if overflowed {
            return Err(MathError::Overflowed);
        }

        let (result, overflowed) = numerator.overflowing_div(MulIndex::ONE.0);
        if overflowed {
            return Err(MathError::Overflowed);
        }

        Ok(Quantity(self.symbol(), result))
    }
}

impl SafeMul<Quantity> for MulIndex {
    type Output = Quantity;

    fn safe_mul(self, rhs: Quantity) -> Result<Self::Output, MathError> {
        rhs.safe_mul(self)
    }
}

pub fn symbol<T: Config>(asset: ChainAsset) -> Symbol {
    // XXX lookup in storage
    Symbol(
        ['E', 'T', 'H', NIL, NIL, NIL, NIL, NIL, NIL, NIL, NIL, NIL],
        18,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETH: Symbol = Symbol(
        ['E', 'T', 'H', NIL, NIL, NIL, NIL, NIL, NIL, NIL, NIL, NIL],
        18,
    );

    #[test]
    fn test_scale_codec() {
        let a = Quantity(CASH, 3000000);
        let encoded = a.encode();
        let decoded = Decode::decode(&mut encoded.as_slice());
        let b = decoded.expect("value did not decode");
        assert_eq!(a, b);
    }
}