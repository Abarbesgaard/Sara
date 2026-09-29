pub(super) enum DoneGate {
    Ok,
    Refuse(String),
    Warn(String),
}
