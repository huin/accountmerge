/// Account name, provided by the bank.
pub const ACCOUNT_TAG: &str = "account";
/// Bank identifier/name, provided by the importer.
pub const BANK_TAG: &str = "bank";
/// Indicates that the posting's account name is known.
pub const CANONICAL_TAG: &str = "canonical";
/// Prefix for a tag key of a fingerprint hash/identifier produced by the
/// importer. The key and value for this must be consistent upon each re-import
/// for any given posting that has it.
pub const FINGERPRINT_TAG_PREFIX: &str = "fp";
