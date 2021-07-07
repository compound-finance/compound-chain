use our_std::{convert::TryInto, str::FromStr};
use serde::{de, ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    chains::{Chain, ChainAccount, ChainAsset, Ethereum},
    symbol::Symbol,
};

// For using in GenesisConfig / ChainSpec JSON.

// ChainAccount

struct ChainAccountVisitor;

impl<'de> de::Visitor<'de> for ChainAccountVisitor {
    type Value = ChainAccount;

    fn expecting(&self, formatter: &mut our_std::fmt::Formatter) -> our_std::fmt::Result {
        formatter.write_str("a string of the form <chain>:<address>")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        ChainAccount::from_str(value).map_err(|_| E::custom("bad account"))
    }
}

impl<'de> de::Deserialize<'de> for ChainAccount {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        de.deserialize_any(ChainAccountVisitor)
    }
}

impl Serialize for ChainAccount {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s: String = (*self).into();
        ser.serialize_str(&s)
    }
}

// ChainAsset

struct ChainAssetVisitor;

impl<'de> de::Visitor<'de> for ChainAssetVisitor {
    type Value = ChainAsset;

    fn expecting(&self, formatter: &mut our_std::fmt::Formatter) -> our_std::fmt::Result {
        formatter.write_str("a string of the form <chain>:<address>")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        ChainAsset::from_str(value).map_err(|_| E::custom("bad asset"))
    }
}

impl<'de> de::Deserialize<'de> for ChainAsset {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        de.deserialize_any(ChainAssetVisitor)
    }
}

impl Serialize for ChainAsset {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s: String = (*self).into();
        ser.serialize_str(&s)
    }
}

// Symbol

struct SymbolVisitor;

impl<'de> de::Visitor<'de> for SymbolVisitor {
    type Value = Symbol;

    fn expecting(&self, formatter: &mut our_std::fmt::Formatter) -> our_std::fmt::Result {
        formatter.write_str("a short string representing the token symbol")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Symbol::from_str(value).map_err(|_| E::custom("bad symmbol"))
    }
}

impl<'de> de::Deserialize<'de> for Symbol {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        de.deserialize_any(SymbolVisitor)
    }
}

impl Serialize for Symbol {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s: String = (*self).into();
        ser.serialize_str(&s)
    }
}
