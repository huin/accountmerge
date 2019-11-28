/// Account name, provided by the bank.
pub const ACCOUNT: &str = "account";
/// Bank identifier/name, provided by the importer.
pub const BANK: &str = "bank";
/// Tag indicating that an importer has marked the posting as *not* being of the
/// account whose data is being imported. That is, it's a posting for an amount
/// against another account.
pub const IMPORT_PEER: &str = "import-peer";
/// Tag indicating that an importer has marked the posting as being of the
/// account whose data is being imported.
pub const IMPORT_SELF: &str = "import-self";
/// Indicates that the posting's account name is unknown.
pub const UNKNOWN_ACCOUNT: &str = "unknown-account";

/// Prefix for a fingerprint tag applied by merging for postings that are
/// candidates for merging from another source.
pub const CANDIDATE_FP_PREFIX: &str = "candidate-";
/// Prefix for a tag key of a fingerprint hash/identifier produced by the
/// importer. The key and value for this must be consistent upon each re-import
/// for any given posting that has it.
pub const FINGERPRINT_PREFIX: &str = "fp-";

/// Key for a key-value tag on a transaction that specifies where it came from.
pub const TRANSACTION_SOURCE_KEY: &str = "source-file";
