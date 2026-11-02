/// How an anchored file compares with the fingerprint stored when it was anchored.
#[derive(Clone, Debug, PartialEq)]
pub enum AnchorState {
    Fresh,
    Drifted(f64),
    Missing,
    Unknown,
}

impl AnchorState {
    pub fn is_stale(&self) -> bool {
        matches!(self, AnchorState::Missing | AnchorState::Drifted(_))
    }
}
