//! Volume management commands
//!
//! Commands for mounting and managing volumes.
//! Mounts remote VM directories locally using SSHFS.

mod list;
mod mount;
mod sync;
mod unmount;

// Re-export public functions
pub use list::{list, status};
pub use mount::mount;
pub use unmount::{remount, unmount};
