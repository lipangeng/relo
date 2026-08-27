use super::model::{Plan, Scope, Snapshot};
use anyhow::Result;

pub(super) struct LockGuard;

pub(super) fn read(_scope: Scope) -> Result<Snapshot> {
    unreachable!("platform support is checked before Windows environment access")
}

pub(super) fn lock(_scope: Scope) -> Result<LockGuard> {
    unreachable!("platform support is checked before Windows environment access")
}

pub(super) fn apply(_scope: Scope, _plan: &Plan) -> Result<()> {
    unreachable!("platform support is checked before Windows environment access")
}

pub(super) fn broadcast() -> Result<()> {
    unreachable!("platform support is checked before Windows environment access")
}
