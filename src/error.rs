use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReloError {
    #[error("not a relo root: {0}")]
    NotRoot(String),
    #[error("missing releases directory: {0}")]
    MissingReleases(String),
    #[error("active exists but is not a symlink: {0}")]
    ActiveNotSymlink(String),
    #[error("active points to missing release: {0}")]
    ActiveMissing(String),
    #[error("active points to invalid target: {0}")]
    ActiveInvalidTarget(String),
    #[error("invalid release directory: {0}")]
    InvalidRelease(String),
    #[error("no release matches: {0}")]
    NoMatch(String),
    #[error("ambiguous release: {expr}\nmatched:\n{matches}\nplease specify full release name")]
    Ambiguous { expr: String, matches: String },
    #[error("no active release")]
    NoActive,
}
