pub mod nationwide;

/// Account name, provided by the bank.
pub const ACCOUNT_TAG: &str = "account";
/// Bank identifier/name, provided by the importer.
pub const BANK_TAG: &str = "bank";
/// Fingerprint hash/identifier provided by the importer. The value for this
/// must be consistent upon each re-import for any given posting that has it.
pub const FINGERPRINT_TAG: &str = "fingerprint";
/// Transaction type field, provided by the bank.
pub const TRANSACTION_TYPE_TAG: &str = "trn_type";

pub const EXPENSES_UNKNOWN: &str = "expenses:unknown";
pub const INCOME_UNKNOWN: &str = "income:unknown";
