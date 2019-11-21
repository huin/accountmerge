use ledger_parser::{Ledger, Posting, Transaction};

use crate::comment::Comment;

/// TransactionComment is a `Transaction` with the comment string (if any) moved
/// out as a `Comment`
pub struct TransactionComment {
    pub trn: Transaction,
    pub comment: Comment,
}

impl From<Transaction> for TransactionComment {
    fn from(mut trn: Transaction) -> Self {
        let comment = Comment::from_opt_string(&trn.comment);
        trn.comment = None;
        Self { trn, comment }
    }
}

impl Into<Transaction> for TransactionComment {
    fn into(mut self) -> Transaction {
        self.trn.comment = self.comment.into_opt_comment();
        self.trn
    }
}

/// A `TransactionComment` paired with its `PostingComment`s.
///
/// Typically for use at the input/output boundary of processing a journal.
pub struct TransactionPostings {
    pub trn: TransactionComment,
    pub posts: Vec<PostingComment>,
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
        let posts: Vec<PostingComment> = raw_posts.into_iter().map(Into::into).collect();
        let trn: TransactionComment = raw_trn.into();
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

/// PostingComment is a `Posting` with the comment string (if any) moved out as
/// a `Comment`
pub struct PostingComment {
    pub post: Posting,
    pub comment: Comment,
}

impl From<Posting> for PostingComment {
    fn from(mut post: Posting) -> Self {
        let comment = Comment::from_opt_string(&post.comment);
        post.comment = None;
        Self { post, comment }
    }
}

impl Into<Posting> for PostingComment {
    fn into(mut self) -> Posting {
        self.post.comment = self.comment.into_opt_comment();
        self.post
    }
}
