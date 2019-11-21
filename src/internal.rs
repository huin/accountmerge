//! Internal wrapper types for `Posting` and `Transaction`.

use ledger_parser::{Ledger, Posting, Transaction};

use crate::comment::Comment;

/// TransactionInternal is a `Transaction` with the comment string (if any) moved
/// out as a `Comment`.
pub struct TransactionInternal {
    pub trn: Transaction,
    pub comment: Comment,
}

impl From<Transaction> for TransactionInternal {
    fn from(mut trn: Transaction) -> Self {
        let comment = Comment::from_opt_string(&trn.comment);
        trn.comment = None;
        Self { trn, comment }
    }
}

impl Into<Transaction> for TransactionInternal {
    fn into(mut self) -> Transaction {
        self.trn.comment = self.comment.into_opt_comment();
        self.trn
    }
}

/// A `TransactionInternal` paired with its `PostingInternal`s.
///
/// Typically for use at the input/output boundary of processing a journal.
pub struct TransactionPostings {
    pub trn: TransactionInternal,
    pub posts: Vec<PostingInternal>,
}

impl TransactionPostings {
    pub fn put_into_ledger(ledger: &mut Ledger, trns: Vec<Self>) {
        ledger.transactions = trns.into_iter().map(Into::into).collect();
    }

    pub fn take_from_ledger(ledger: &mut Ledger) -> Vec<Self> {
        let raw_trns = std::mem::replace(&mut ledger.transactions, Vec::new());
        raw_trns.into_iter().map(Into::into).collect()
    }
}

impl From<Transaction> for TransactionPostings {
    fn from(mut raw_trn: Transaction) -> Self {
        let raw_posts = std::mem::replace(&mut raw_trn.postings, Vec::new());
        let posts: Vec<PostingInternal> = raw_posts.into_iter().map(Into::into).collect();
        let trn: TransactionInternal = raw_trn.into();
        Self { trn, posts }
    }
}

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
#[derive(Clone)]
pub struct PostingInternal {
    pub post: Posting,
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

impl From<Posting> for PostingInternal {
    fn from(mut post: Posting) -> Self {
        let comment = Comment::from_opt_string(&post.comment);
        post.comment = None;
        Self { post, comment }
    }
}

impl Into<Posting> for PostingInternal {
    fn into(mut self) -> Posting {
        self.post.comment = self.comment.into_opt_comment();
        self.post
    }
}
