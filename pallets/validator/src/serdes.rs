use crate::types::ValidatorKeys;
use our_std::str::FromStr;
use serde::{de, ser::SerializeSeq, Deserializer, Serialize, Serializer};

// ValidatorKeys & Vec<ValidatorKeys>

impl<'de> de::Deserialize<'de> for ValidatorKeys {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            SubstrateId,
            EthAddress,
        }
        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;
                impl<'de> de::Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut our_std::fmt::Formatter,
                    ) -> our_std::fmt::Result {
                        formatter.write_str("`substrate_id` or `eth_address`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "substrate_id" => Ok(Field::SubstrateId),
                            "eth_address" => Ok(Field::EthAddress),
                            _ => Err(de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct ValidatorKeysVisitor;
        impl<'de> de::Visitor<'de> for ValidatorKeysVisitor {
            type Value = ValidatorKeys;

            fn expecting(&self, formatter: &mut our_std::fmt::Formatter) -> our_std::fmt::Result {
                formatter.write_str("struct ValidatorKeys")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut substrate_id = None;
                let mut eth_address = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::SubstrateId => {
                            if substrate_id.is_some() {
                                return Err(de::Error::duplicate_field("substrate_id"));
                            }
                            let s_id: [u8; 32] = map.next_value()?;
                            substrate_id = Some(
                                s_id.try_into()
                                    .map_err(|_| de::Error::custom("bad substrate id"))?,
                            );
                        }
                        Field::EthAddress => {
                            if eth_address.is_some() {
                                return Err(de::Error::duplicate_field("eth_address"));
                            }
                            let addr: String = map.next_value()?;
                            eth_address = Some(
                                gateway_crypto::eth_str_to_address(&addr)
                                    .map_err(|_| de::Error::custom("bad eth address"))?,
                            );
                        }
                    }
                }
                let substrate_id =
                    substrate_id.ok_or_else(|| de::Error::missing_field("substrate_id"))?;
                let eth_address =
                    eth_address.ok_or_else(|| de::Error::missing_field("eth_address"))?;
                Ok(ValidatorKeys {
                    substrate_id,
                    eth_address,
                })
            }
        }

        const FIELDS: &'static [&'static str] = &["substrate_id", "eth_address"];
        de.deserialize_struct("ValidatorKeys", FIELDS, ValidatorKeysVisitor)
    }
}

impl Serialize for ValidatorKeys {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = ser.serialize_map(Some(2))?;
        map.serialize_key("substrate_id")?;
        map.serialize_value(&<[u8; 32]>::from(self.substrate_id.clone()))?;
        map.serialize_key("eth_address")?;
        map.serialize_value(&gateway_crypto::eth_address_string(&self.eth_address))?;
        map.end()
    }
}
