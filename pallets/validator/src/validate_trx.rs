use crate::{internal, Call, Config, Validators};
use codec::Encode;
use frame_support::storage::{IterableStorageMap, StorageDoubleMap, StorageValue};
use our_std::{log, RuntimeDebug};
use sp_runtime::transaction_validity::{TransactionSource, TransactionValidity, ValidTransaction};

#[derive(Eq, PartialEq, RuntimeDebug, Clone, Copy)]
pub enum ValidationError {
    InvalidCall,
}

pub fn check_validation_failure<T: Config>(
    call: &Call<T>,
    res: Result<TransactionValidity, ValidationError>,
) -> Result<TransactionValidity, ValidationError> {
    if let Err(err) = res {
        log!("validate_unsigned: call = {:#?}, error = {:#?}", call, err);
    }
    res
}

pub fn validate_unsigned<T: Config>(
    source: TransactionSource,
    call: &Call<T>,
) -> Result<TransactionValidity, ValidationError> {
    match call {
        _ => Err(ValidationError::InvalidCall),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core::validator_sign, tests::*, Call};

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
