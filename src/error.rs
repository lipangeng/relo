use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReloError {
    #[error("not a relo context: {0}")]
    NotRoot(String),
    #[error("missing releases directory: {0}")]
    MissingReleases(String),
    #[error("active exists but is not a managed link: {0}")]
    ActiveNotManagedLink(String),
    #[error("active points to missing release: {0}")]
    ActiveMissing(String),
    #[error("active points to invalid target: {0}")]
    ActiveInvalidTarget(String),
    #[error("cannot determine latest: no comparable numeric releases\navailable:\n  {0}\nplease specify full release name")]
    NoComparableRelease(String),
    #[error("no release matches: {0}")]
    NoMatch(String),
    #[error("ambiguous release: {expr}\nmatched:\n{matches}\nplease specify full release name")]
    Ambiguous { expr: String, matches: String },
}
