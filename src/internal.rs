//! Internal wrapper types for `Posting` and `Transaction`.

use anyhow::{Result, anyhow};
use ledger_parser::{Ledger, LedgerItem, Posting, Transaction};

use crate::{
    comment::Comment,
    ledgerutil::{self, simple_posting_amount},
};

/// TransactionInternal is a `Transaction` with the comment string (if any) moved
/// out as a `Comment`.
#[derive(Clone, Debug)]
pub struct TransactionInternal {
    pub raw: Transaction,
    pub comment: Comment,
}

impl TryFrom<Transaction> for TransactionInternal {
    type Error = anyhow::Error;

    fn try_from(mut raw: Transaction) -> Result<Self> {
        let tags = take_tags(&mut raw.posting_metadata);
        let comment = Comment::from_tags_and_comment(tags, raw.comment)?;
        raw.comment = None;
        Ok(Self { raw, comment })
    }
}

#[allow(clippy::from_over_into)] // Can't implement `From for Transaction` from other crate.
impl Into<Transaction> for TransactionInternal {
    fn into(mut self) -> Transaction {
        self.raw.comment = self.comment.into_opt_comment();
        self.raw
    }
}

/// A `TransactionInternal` paired with its `PostingInternal`s.
///
/// Typically for use at the input/output boundary of processing a journal.
#[derive(Clone, Debug)]
pub struct TransactionPostings {
    pub trn: TransactionInternal,
    pub posts: Vec<PostingInternal>,
}

impl TransactionPostings {
    pub fn from_ledger(ledger: Ledger) -> Result<Vec<Self>> {
        ledger
            .items
            .into_iter()
            .filter_map(|item| match item {
                LedgerItem::Transaction(trn) => Some(TransactionPostings::try_from(trn)),
                LedgerItem::EmptyLine => None,
                other => Some(Err(anyhow!(
                    "unhandled item type in ledger (these are not yet handled): {:?}",
                    other
                ))),
            })
            .collect()
    }

    pub fn into_ledger(trns: Vec<Self>) -> Ledger {
        ledgerutil::ledger_from_transactions(trns.into_iter().map(|trn| trn.into()))
    }
}

impl TryFrom<Transaction> for TransactionPostings {
    type Error = anyhow::Error;

    fn try_from(mut raw_trn: Transaction) -> Result<Self> {
        let raw_posts = std::mem::take(&mut raw_trn.postings);
        let posts = raw_posts
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<PostingInternal>>>()?;
        let trn: TransactionInternal = raw_trn.try_into()?;
        Ok(Self { trn, posts })
    }
}

#[allow(clippy::from_over_into)] // Can't implement `From for Transaction` from other crate.
impl Into<Transaction> for TransactionPostings {
    fn into(self) -> Transaction {
        let raw_posts: Vec<Posting> = self.posts.into_iter().map(Into::into).collect();
        let mut raw_trn: Transaction = self.trn.into();
        raw_trn.postings = raw_posts;
        raw_trn
    }
}

/// PostingInternal is a `Posting` with the comment string (if any) moved out as
/// a `Comment`
#[derive(Clone, Debug)]
pub struct PostingInternal {
    pub raw: Posting,
    pub comment: Comment,
}

impl PostingInternal {
    /// clone_into_posting is a shorthand for `self.clone.into()`, but without
    /// having to specify the type parameters.
    ///
    /// It is naturally slightly expensive, and intended mostly for generating
    /// error messages using the `Display` implementation of `Posting`.
    pub fn clone_into_posting(&self) -> Posting {
        self.clone().into()
    }
}

impl TryFrom<Posting> for PostingInternal {
    type Error = anyhow::Error;

    fn try_from(mut raw: Posting) -> Result<Self> {
        let tags = take_tags(&mut raw.metadata);
        let comment = Comment::from_tags_and_comment(tags, raw.comment)?;
        raw.comment = None;
        Ok(Self { raw, comment })
    }
}

#[allow(clippy::from_over_into)] // Can't implement `From for Posting` from other crate.
impl Into<Posting> for PostingInternal {
    fn into(mut self) -> Posting {
        self.raw.comment = self.comment.into_opt_comment();
        self.raw
    }
}

pub struct PostingInternalBuilder {
    posting: PostingInternal,
}

impl PostingInternalBuilder {
    pub fn new(account: String) -> Self {
        Self {
            posting: PostingInternal {
                raw: Posting {
                    account,
                    reality: ledger_parser::Reality::Real,
                    amount: None,
                    balance: None,
                    status: None,
                    comment: None,
                    metadata: ledgerutil::empty_posting_metadata(),
                },
                comment: Comment::default(),
            },
        }
    }

    pub fn with_simple_amount(self, amount: ledger_parser::Amount) -> Self {
        self.with_amount(simple_posting_amount(amount))
    }

    pub fn with_amount(mut self, amount: ledger_parser::PostingAmount) -> Self {
        self.posting.raw.amount = Some(amount);
        self
    }

    pub fn with_opt_balance(mut self, opt_balance: Option<ledger_parser::Balance>) -> Self {
        self.posting.raw.balance = opt_balance;
        self
    }

    pub fn with_balance(self, balance: ledger_parser::Balance) -> Self {
        self.with_opt_balance(Some(balance))
    }

    pub fn with_status(mut self, status: ledger_parser::TransactionStatus) -> Self {
        self.posting.raw.status = Some(status);
        self
    }

    pub fn with_comment(mut self, comment: Comment) -> Self {
        self.posting.comment = comment;
        self
    }

    pub fn build(self) -> PostingInternal {
        self.posting
    }
}

fn take_tags(metadata: &mut ledger_parser::PostingMetadata) -> Vec<ledger_parser::Tag> {
    let mut tags = Vec::new();
    std::mem::swap(&mut metadata.tags, &mut tags);
    tags
}
