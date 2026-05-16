// Install-channel detection.
//
// Until this app has a winget package, every install is treated as
// "portable" — meaning self-update applies via the inline-cmd handoff.
// When a winget package exists, restore the path-probe code from git
// history (see commit before Phase 6).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Portable,
    Winget,
}

pub fn current() -> Channel {
    Channel::Portable
}
