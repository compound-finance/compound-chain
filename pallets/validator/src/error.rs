use codec::{Decode, Encode};
use frame_support;
use our_std::Debuggable;

use types_derive::Types;

/// Errors coming from the price oracle.
#[derive(Copy, Clone, Eq, PartialEq, Encode, Decode, Debuggable, Types)]
pub enum ValidatorError {}

impl From<ValidatorError> for frame_support::dispatch::DispatchError {
    fn from(err: ValidatorError) -> frame_support::dispatch::DispatchError {
        let (index, error, message) = match err {};
        frame_support::dispatch::DispatchError::Module {
            index,
            error,
            message: Some(message),
        }
    }
}
