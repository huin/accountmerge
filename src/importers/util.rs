use ledger_parser::Amount;

use crate::accounts::{EXPENSES_UNKNOWN, INCOME_UNKNOWN};
use crate::fingerprint::{Fingerprint, FingerprintBuilder};

pub fn negate_amount(amt: Amount) -> Amount {
    Amount {
        quantity: -amt.quantity,
        commodity: amt.commodity,
    }
}

pub struct AccountAmount {
    pub account: String,
    pub amount: Amount,
}

pub struct TransactionHalves {
    pub self_: AccountAmount,
    pub peer: AccountAmount,
}

pub fn self_and_peer_account_amount(
    self_amount: Amount,
    self_account: String,
) -> TransactionHalves {
    let peer_account = if self_amount.quantity.is_sign_negative() {
        EXPENSES_UNKNOWN
    } else {
        INCOME_UNKNOWN
    };

    TransactionHalves {
        self_: AccountAmount {
            account: self_account,
            amount: self_amount.clone(),
        },
        peer: AccountAmount {
            account: peer_account.to_string(),
            amount: negate_amount(self_amount),
        },
    }
}

pub struct FingerprintHalves {
    pub self_: Fingerprint,
    pub peer: Fingerprint,
}

pub fn self_and_peer_fingerprints(fpb: FingerprintBuilder) -> FingerprintHalves {
    FingerprintHalves {
        self_: fpb.clone().with("self").build(),
        peer: fpb.with("peer").build(),
    }
}
