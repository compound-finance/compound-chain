use crate::{
    chains::{Chain, ChainAccount, Ethereum},
    internal,
    notices::EncodeNotice,
    reason::Reason,
    AllowedNextCodeHash, Call, Config, Notices, Validators,
};
use codec::Encode;
use frame_support::storage::{IterableStorageMap, StorageDoubleMap, StorageValue};
use our_std::RuntimeDebug;
use sp_runtime::transaction_validity::{TransactionSource, TransactionValidity, ValidTransaction};

#[derive(Eq, PartialEq, RuntimeDebug)]
pub enum ValidationError {
    InvalidInternalOnly,
    InvalidNextCode,
    InvalidValidator,
    InvalidSignature,
    InvalidCall,
    InvalidPriceSignature,
    InvalidPrice(Reason),
    UnknownNotice,
}

pub fn validate_unsigned<T: Config>(
    source: TransactionSource,
    call: &Call<T>,
) -> Result<TransactionValidity, ValidationError> {
    match call {
        Call::set_miner(_miner) => match source {
            TransactionSource::Local => Ok(ValidTransaction::with_tag_prefix("Gateway::set_miner")
                .longevity(1)
                .build()),
            _ => Err(ValidationError::InvalidInternalOnly),
        },
        Call::set_next_code_via_hash(next_code) => {
            let hash = <Ethereum as Chain>::hash_bytes(&next_code);

            if AllowedNextCodeHash::get() == Some(hash) {
                Ok(
                    ValidTransaction::with_tag_prefix("Gateway::set_next_code_via_hash")
                        .priority(100)
                        .longevity(32)
                        .and_provides(hash)
                        .propagate(true)
                        .build(),
                )
            } else {
                Err(ValidationError::InvalidNextCode)
            }
        }
        Call::receive_event(event_id, event, signature) => {
            if let Ok(signer) = gateway_crypto::eth_recover(&event.encode()[..], &signature, false)
            {
                let validators: Vec<_> = Validators::iter().map(|v| v.1.eth_address).collect();
                if validators.contains(&signer) {
                    Ok(ValidTransaction::with_tag_prefix("Gateway::receive_event")
                        .priority(100)
                        .longevity(32)
                        .and_provides((event_id, signature))
                        .propagate(true)
                        .build())
                } else {
                    Err(ValidationError::InvalidValidator)
                }
            } else {
                Err(ValidationError::InvalidSignature)
            }
        }
        Call::exec_trx_request(request, signature, nonce) => {
            let signer_res = signature
                .recover_account(&internal::exec_trx_request::prepend_nonce(request, *nonce)[..]);

            // TODO: We might want to check existential value of signer

            match (signer_res, nonce) {
                (Err(_e), _) => Err(ValidationError::InvalidSignature),
                (Ok(sender), 0) => Ok(ValidTransaction::with_tag_prefix(
                    "Gateway::exec_trx_request",
                )
                .priority(100)
                .longevity(32)
                .and_provides((sender, 0))
                .propagate(true)
                .build()),
                (Ok(sender), _) => Ok(ValidTransaction::with_tag_prefix(
                    "Gateway::exec_trx_request",
                )
                .priority(100)
                .longevity(32)
                .and_provides((sender, nonce))
                .and_requires((sender, nonce - 1))
                .propagate(true)
                .build()),
            }
        }
        Call::post_price(payload, signature) => {
            if internal::oracle::check_signature::<T>(&payload, &signature) == Ok(true) {
                match source {
                    TransactionSource::Local => {
                        Ok(ValidTransaction::with_tag_prefix("Gateway::post_price")
                            .priority(100)
                            .and_provides(signature)
                            .build())
                    }
                    _ => match internal::oracle::get_and_check_parsed_price::<T>(payload) {
                        Ok(_) => Ok(ValidTransaction::with_tag_prefix("Gateway::post_price")
                            .priority(100)
                            .and_provides(signature)
                            .propagate(true)
                            .build()),
                        Err(reason) => Err(ValidationError::InvalidPrice(reason)),
                    },
                }
            } else {
                Err(ValidationError::InvalidPriceSignature)
            }
        }
        Call::publish_signature(chain_id, notice_id, signature) => {
            let notice = Notices::get(chain_id, notice_id).ok_or(ValidationError::UnknownNotice)?;
            let signer = signature
                .recover(&notice.encode_notice())
                .map_err(|_| ValidationError::InvalidSignature)?;

            if Validators::iter().any(|(_, v)| ChainAccount::Eth(v.eth_address) == signer) {
                Ok(
                    ValidTransaction::with_tag_prefix("Gateway::publish_signature")
                        .priority(100)
                        .longevity(32)
                        .and_provides(signature)
                        .propagate(true)
                        .build(),
                )
            } else {
                Err(ValidationError::InvalidValidator)
            }
        }
        _ => Err(ValidationError::InvalidCall),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chains::{
            Chain, ChainAccount, ChainAccountSignature, ChainId, ChainSignature,
            ChainSignatureList, Ethereum,
        },
        events::{ChainLogEvent, ChainLogId},
        mock::*,
        notices::{ExtractionNotice, Notice, NoticeId, NoticeState},
        reason,
        symbol::Ticker,
        types::{ReporterSet, ValidatorKeys, ValidatorSig},
        Call, NoticeStates, PriceReporters, PriceTimes, Validators,
    };
    use ethereum_client::{events::EthereumEvent::Lock, EthereumLogEvent};
    use frame_support::storage::StorageMap;

    use sp_core::crypto::AccountId32;

    #[test]
    fn test_set_miner_external() {
        new_test_ext().execute_with(|| {
            let miner = ChainAccount::Eth([0u8; 20]);
            assert_eq!(
                validate_unsigned(
                    TransactionSource::External {},
                    &Call::set_miner::<Test>(miner),
                ),
                Err(ValidationError::InvalidInternalOnly)
            );
        });
    }

    #[test]
    fn test_set_miner_local() {
        new_test_ext().execute_with(|| {
            let miner = ChainAccount::Eth([0u8; 20]);
            let exp = ValidTransaction::with_tag_prefix("Gateway::set_miner")
                .longevity(1)
                .build();

            assert_eq!(
                validate_unsigned(TransactionSource::Local {}, &Call::set_miner::<Test>(miner),),
                Ok(exp)
            );
        });
    }

    #[test]
    fn test_set_next_code_via_hash_not_exists() {
        new_test_ext().execute_with(|| {
            let next_code: Vec<u8> = [0u8; 10].into();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::set_next_code_via_hash::<Test>(next_code),
                ),
                Err(ValidationError::InvalidNextCode)
            );
        });
    }

    #[test]
    fn test_set_next_code_via_hash_exists_mismatch() {
        new_test_ext().execute_with(|| {
            AllowedNextCodeHash::put([0u8; 32]);
            let next_code: Vec<u8> = [0u8; 10].into();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::set_next_code_via_hash::<Test>(next_code),
                ),
                Err(ValidationError::InvalidNextCode)
            );
        });
    }

    #[test]
    fn test_set_next_code_via_hash_exists_match() {
        new_test_ext().execute_with(|| {
            let next_code: Vec<u8> = [0u8; 10].into();
            let hash = <Ethereum as Chain>::hash_bytes(&next_code);
            AllowedNextCodeHash::put(hash);
            let exp = ValidTransaction::with_tag_prefix("Gateway::set_next_code_via_hash")
                .priority(100)
                .longevity(32)
                .and_provides(hash)
                .propagate(true)
                .build();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::set_next_code_via_hash::<Test>(next_code),
                ),
                Ok(exp)
            );
        });
    }

    #[test]
    fn test_receive_event_recover_failure() {
        new_test_ext().execute_with(|| {
            let event_id: ChainLogId = ChainLogId::Eth(1, 1);
            let event: ChainLogEvent = ChainLogEvent::Eth(EthereumLogEvent {
                block_hash: [0u8; 32],
                block_number: 1,
                transaction_index: 1,
                log_index: 1,
                event: Lock {
                    asset: [0u8; 20],
                    sender: [0u8; 20],
                    chain: String::from("ETH"),
                    recipient: [0u8; 32],
                    amount: 500,
                },
            });
            let signature: ValidatorSig = [0u8; 65];

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::receive_event::<Test>(event_id, event, signature)
                ),
                Err(ValidationError::InvalidSignature)
            );
        });
    }

    #[test]
    fn test_receive_event_not_a_validator() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();

            let event_id: ChainLogId = ChainLogId::Eth(1, 1);
            let event: ChainLogEvent = ChainLogEvent::Eth(EthereumLogEvent {
                block_hash: [0u8; 32],
                block_number: 1,
                transaction_index: 1,
                log_index: 1,
                event: Lock {
                    asset: [0u8; 20],
                    sender: [0u8; 20],
                    chain: String::from("ETH"),
                    recipient: [0u8; 32],
                    amount: 500,
                },
            });
            let eth_signature = match event.sign_event().unwrap() {
                ChainSignature::Eth(s) => s,
                _ => panic!("absurd"),
            };

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::receive_event::<Test>(event_id, event, eth_signature)
                ),
                Err(ValidationError::InvalidValidator)
            );
        });
    }

    #[test]
    fn test_receive_event_is_validator() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();
            let substrate_id = AccountId32::new([0u8; 32]);
            let eth_address = <Ethereum as Chain>::signer_address().unwrap();
            Validators::insert(
                substrate_id.clone(),
                ValidatorKeys {
                    substrate_id,
                    eth_address,
                },
            );

            let event_id: ChainLogId = ChainLogId::Eth(1, 1);
            let event: ChainLogEvent = ChainLogEvent::Eth(EthereumLogEvent {
                block_hash: [0u8; 32],
                block_number: 1,
                transaction_index: 1,
                log_index: 1,
                event: Lock {
                    asset: [0u8; 20],
                    sender: [0u8; 20],
                    chain: String::from("ETH"),
                    recipient: [0u8; 32],
                    amount: 500,
                },
            });
            let eth_signature = match event.sign_event().unwrap() {
                ChainSignature::Eth(s) => s,
                _ => panic!("absurd"),
            };
            let exp = ValidTransaction::with_tag_prefix("Gateway::receive_event")
                .priority(100)
                .longevity(32)
                .and_provides((event_id, eth_signature))
                .propagate(true)
                .build();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::receive_event::<Test>(event_id, event, eth_signature)
                ),
                Ok(exp)
            );
        });
    }

    #[test]
    fn test_exec_trx_request_nonce_zero() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();
            let request: Vec<u8> = String::from("Hello").as_bytes().into();
            let nonce = 0;
            let full_request: Vec<u8> = format!("\x19Ethereum Signed Message:\n70:Hello")
                .as_bytes()
                .into();
            let eth_address = <Ethereum as Chain>::signer_address().unwrap();
            let eth_key_id =
                runtime_interfaces::validator_config_interface::get_eth_key_id().unwrap();
            let signature_raw =
                runtime_interfaces::keyring_interface::sign_one(full_request, eth_key_id).unwrap();

            let signature = ChainAccountSignature::Eth(eth_address, signature_raw);

            let exp = ValidTransaction::with_tag_prefix("Gateway::exec_trx_request")
                .priority(100)
                .longevity(32)
                .and_provides((ChainAccount::Eth(eth_address), 0))
                .propagate(true)
                .build();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::exec_trx_request::<Test>(request, signature, nonce),
                ),
                Ok(exp)
            );
        });
    }

    #[test]
    fn test_exec_trx_request_nonce_nonzero() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();
            let request: Vec<u8> = String::from("Hello").as_bytes().into();
            let nonce = 5;
            let full_request: Vec<u8> = format!("\x19Ethereum Signed Message:\n75:Hello")
                .as_bytes()
                .into();
            let eth_address = <Ethereum as Chain>::signer_address().unwrap();
            let eth_key_id =
                runtime_interfaces::validator_config_interface::get_eth_key_id().unwrap();
            let signature_raw =
                runtime_interfaces::keyring_interface::sign_one(full_request, eth_key_id).unwrap();

            let signature = ChainAccountSignature::Eth(eth_address, signature_raw);

            let exp = ValidTransaction::with_tag_prefix("Gateway::exec_trx_request")
                .priority(100)
                .longevity(32)
                .and_requires((ChainAccount::Eth(eth_address), 4))
                .and_provides((ChainAccount::Eth(eth_address), 5))
                .propagate(true)
                .build();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::exec_trx_request::<Test>(request, signature, nonce),
                ),
                Ok(exp)
            );
        });
    }

    #[test]
    fn test_post_price_invalid_signature() {
        new_test_ext().execute_with(|| {
            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::post_price::<Test>(vec![], vec![]),
                ),
                Err(ValidationError::InvalidPriceSignature)
            );
        });
    }

    #[test]
    fn test_post_price_stale() {
        new_test_ext().execute_with(|| {
            PriceReporters::put(ReporterSet(vec![[133, 97, 91, 7, 102, 21, 49, 124, 128, 241, 76, 186, 214, 80, 30, 236, 3, 28, 213, 28]]));
            let ticker = Ticker::new("BTC");
            PriceTimes::insert(ticker, 999999999999999);
            let msg = hex_literal::hex!("0000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000005fec975800000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000688e4cda00000000000000000000000000000000000000000000000000000000000000006707269636573000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034254430000000000000000000000000000000000000000000000000000000000");
            let sig = hex_literal::hex!("69538bfa1a2097ea206780654d7baac3a17ee57547ee3eeb5d8bcb58a2fcdf401ff8834f4a003193f24224437881276fe76c8e1c0a361081de854457d41d0690000000000000000000000000000000000000000000000000000000000000001c");

            assert_eq!(
                validate_unsigned(
                    TransactionSource::External {},
                    &Call::post_price::<Test>(msg.to_vec(), sig.to_vec()),
                ),
                Err(ValidationError::InvalidPrice(Reason::OracleError(reason::OracleError::StalePrice)))
            );
        });
    }

    #[test]
    fn test_post_price_valid_remote() {
        new_test_ext().execute_with(|| {
            PriceReporters::put(ReporterSet(vec![[133, 97, 91, 7, 102, 21, 49, 124, 128, 241, 76, 186, 214, 80, 30, 236, 3, 28, 213, 28]]));

            let msg = hex_literal::hex!("0000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000005fec975800000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000688e4cda00000000000000000000000000000000000000000000000000000000000000006707269636573000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034254430000000000000000000000000000000000000000000000000000000000");
            let sig = hex_literal::hex!("69538bfa1a2097ea206780654d7baac3a17ee57547ee3eeb5d8bcb58a2fcdf401ff8834f4a003193f24224437881276fe76c8e1c0a361081de854457d41d0690000000000000000000000000000000000000000000000000000000000000001c");

            assert_eq!(
                validate_unsigned(
                    TransactionSource::External {},
                    &Call::post_price::<Test>(msg.to_vec(), sig.to_vec()),
                ),
                Ok(ValidTransaction::with_tag_prefix("Gateway::post_price")
                    .priority(100)
                    .and_provides(sig.to_vec())
                    .propagate(true)
                    .build())
            );
        });
    }

    #[test]
    fn test_post_price_valid_local() {
        new_test_ext().execute_with(|| {
            PriceReporters::put(ReporterSet(vec![[133, 97, 91, 7, 102, 21, 49, 124, 128, 241, 76, 186, 214, 80, 30, 236, 3, 28, 213, 28]]));

            let msg = hex_literal::hex!("0000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000005fec975800000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000688e4cda00000000000000000000000000000000000000000000000000000000000000006707269636573000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034254430000000000000000000000000000000000000000000000000000000000");
            let sig = hex_literal::hex!("69538bfa1a2097ea206780654d7baac3a17ee57547ee3eeb5d8bcb58a2fcdf401ff8834f4a003193f24224437881276fe76c8e1c0a361081de854457d41d0690000000000000000000000000000000000000000000000000000000000000001c");

            assert_eq!(
                validate_unsigned(
                    TransactionSource::Local {},
                    &Call::post_price::<Test>(msg.to_vec(), sig.to_vec()),
                ),
                Ok(ValidTransaction::with_tag_prefix("Gateway::post_price")
                    .priority(100)
                    .and_provides(sig.to_vec())
                    .build())
            );
        });
    }

    #[test]
    fn test_publish_signature_invalid_signature() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();
            let chain_id = ChainId::Eth;
            let notice_id = NoticeId(5, 6);
            let notice = Notice::ExtractionNotice(ExtractionNotice::Eth {
                id: NoticeId(80, 1),
                parent: [3u8; 32],
                asset: [1; 20],
                amount: 100,
                account: [2; 20],
            });
            let mut signature = notice.sign_notice().unwrap();
            let eth_signature = match signature {
                ChainSignature::Eth(ref mut a) => {
                    a[64] = 2;
                    a
                }
                _ => panic!("invalid signature"),
            };
            let signer = <Ethereum as Chain>::signer_address().unwrap();
            let notice_state = NoticeState::Pending {
                signature_pairs: ChainSignatureList::Eth(vec![(signer, *eth_signature)]),
            };
            NoticeStates::insert(chain_id, notice_id, notice_state);
            Notices::insert(chain_id, notice_id, notice);

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::publish_signature::<Test>(chain_id, notice_id, signature),
                ),
                Err(ValidationError::InvalidSignature)
            );
        });
    }

    #[test]
    fn test_publish_signature_invalid_validator() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();
            let chain_id = ChainId::Eth;
            let notice_id = NoticeId(5, 6);
            let notice = Notice::ExtractionNotice(ExtractionNotice::Eth {
                id: NoticeId(80, 1),
                parent: [3u8; 32],
                asset: [1; 20],
                amount: 100,
                account: [2; 20],
            });
            let signature = notice.sign_notice().unwrap();
            let eth_signature = match signature {
                ChainSignature::Eth(a) => a,
                _ => panic!("invalid signature"),
            };
            let signer = <Ethereum as Chain>::signer_address().unwrap();
            let notice_state = NoticeState::Pending {
                signature_pairs: ChainSignatureList::Eth(vec![(signer, eth_signature)]),
            };
            NoticeStates::insert(chain_id, notice_id, notice_state);
            Notices::insert(chain_id, notice_id, notice);

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::publish_signature::<Test>(chain_id, notice_id, signature),
                ),
                Err(ValidationError::InvalidValidator)
            );
        });
    }

    #[test]
    fn test_publish_signature_valid() {
        new_test_ext().execute_with(|| {
            runtime_interfaces::set_validator_config_dev_defaults();
            let chain_id = ChainId::Eth;
            let notice_id = NoticeId(5, 6);
            let notice = Notice::ExtractionNotice(ExtractionNotice::Eth {
                id: NoticeId(80, 1),
                parent: [3u8; 32],
                asset: [1; 20],
                amount: 100,
                account: [2; 20],
            });
            let signature = notice.sign_notice().unwrap();
            let eth_signature = match signature {
                ChainSignature::Eth(a) => a,
                _ => panic!("invalid signature"),
            };
            let signer = <Ethereum as Chain>::signer_address().unwrap();
            let notice_state = NoticeState::Pending {
                signature_pairs: ChainSignatureList::Eth(vec![(signer, eth_signature)]),
            };
            NoticeStates::insert(chain_id, notice_id, notice_state);
            Notices::insert(chain_id, notice_id, notice);
            let substrate_id = AccountId32::new([0u8; 32]);
            Validators::insert(
                substrate_id.clone(),
                ValidatorKeys {
                    substrate_id,
                    eth_address: signer,
                },
            );

            let exp = ValidTransaction::with_tag_prefix("Gateway::publish_signature")
                .priority(100)
                .longevity(32)
                .and_provides(signature)
                .propagate(true)
                .build();

            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::publish_signature::<Test>(chain_id, notice_id, signature),
                ),
                Ok(exp)
            );
        });
    }

    #[test]
    fn test_other() {
        new_test_ext().execute_with(|| {
            assert_eq!(
                validate_unsigned(
                    TransactionSource::InBlock {},
                    &Call::change_validators::<Test>(vec![]),
                ),
                Err(ValidationError::InvalidCall)
            );
        });
    }
}