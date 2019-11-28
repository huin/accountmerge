pub mod cmd;
mod matchset;
mod merger;
mod posting;
mod sources;
mod transaction;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(display = "bad input to merge: {}", reason)]
    Input { reason: String },
    #[fail(display = "internal merge error: {}", reason)]
    Internal { reason: String },
}
