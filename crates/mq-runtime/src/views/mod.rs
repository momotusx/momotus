pub mod addons;
pub mod discover;
pub mod home;
pub mod import;
pub mod library;
pub mod pairing;
pub mod playback;
pub mod search;
pub mod settings;
pub mod title;
pub mod versions;

use mq_library::Table;
use tokio::sync::broadcast::{Receiver, error::RecvError};

/// Waits until the installed addons change: an install, removal, move or
/// switch, a finished import, or a list pulled by sync. False once the
/// library is gone.
pub(crate) async fn addons_changed(changes: &mut Receiver<Vec<Table>>) -> bool {
    loop {
        match changes.recv().await {
            Ok(tables) if tables.iter().any(|t| matches!(t, Table::Addons)) => return true,
            Ok(_) => {}
            // Some changes were missed; the addons may be among them.
            Err(RecvError::Lagged(_)) => return true,
            Err(RecvError::Closed) => return false,
        }
    }
}
