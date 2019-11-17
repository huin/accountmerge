mod matchset;
pub mod merger;
mod posting;
mod transaction;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(display = "bad input to merge: {}", reason)]
    Input { reason: String },
}
