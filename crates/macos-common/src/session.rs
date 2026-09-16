// SPDX-License-Identifier: GPL-3.0-only
//! Session and power actions shared by the applets, mirroring `cosmic-applet-power`.

use logind_zbus::{
    manager::ManagerProxy,
    session::{SessionClass, SessionProxy, SessionType},
    user::UserProxy,
};
use zbus::{Connection, proxy};

#[proxy(
    interface = "com.system76.CosmicSession",
    default_service = "com.system76.CosmicSession",
    default_path = "/com/system76/CosmicSession"
)]
trait CosmicSession {
    fn exit(&self) -> zbus::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Sleep,
    Restart,
    ShutDown,
    Lock,
    LogOut,
}

impl PowerAction {
    /// Argument understood by `cosmic-osd` for its confirmation dialog, if any.
    #[must_use]
    pub const fn osd_confirmation_arg(self) -> Option<&'static str> {
        match self {
            Self::Restart => Some("restart"),
            Self::ShutDown => Some("shutdown"),
            Self::LogOut => Some("log-out"),
            Self::Sleep | Self::Lock => None,
        }
    }

    /// Short, untranslated verb used in logs.
    #[must_use]
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Sleep => "suspend",
            Self::Restart => "restart",
            Self::ShutDown => "shut down",
            Self::Lock => "lock the screen",
            Self::LogOut => "log out",
        }
    }

    /// Perform the action immediately, without confirmation.
    pub async fn perform(self) -> zbus::Result<()> {
        match self {
            Self::Sleep => system_manager().await?.suspend(true).await,
            Self::Restart => system_manager().await?.reboot(true).await,
            Self::ShutDown => system_manager().await?.power_off(true).await,
            Self::Lock => lock().await,
            Self::LogOut => log_out().await,
        }
    }
}

async fn system_manager() -> zbus::Result<ManagerProxy<'static>> {
    let connection = Connection::system().await?;
    ManagerProxy::new(&connection).await
}

async fn lock() -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let manager = ManagerProxy::new(&connection).await?;
    let uid = rustix::process::getuid().as_raw();
    let user_path = manager.get_user(uid).await?;
    let user = UserProxy::builder(&connection)
        .path(user_path)?
        .build()
        .await?;

    // Lock every graphical session of this user, like the upstream power applet.
    let mut locked_any = false;
    for (_, session_path) in user.sessions().await? {
        let Ok(session) = SessionProxy::builder(&connection)
            .path(session_path)?
            .build()
            .await
        else {
            continue;
        };
        if session.class().await == Ok(SessionClass::User)
            && session.type_().await.is_ok_and(|t| t != SessionType::TTY)
            && session.lock().await.is_ok()
        {
            locked_any = true;
        }
    }

    if locked_any {
        Ok(())
    } else {
        Err(zbus::Error::Failure(
            "no graphical session could be locked".into(),
        ))
    }
}

async fn log_out() -> zbus::Result<()> {
    let connection = Connection::session().await?;
    CosmicSessionProxy::new(&connection).await?.exit().await
}

#[cfg(test)]
mod tests {
    use super::PowerAction;

    #[test]
    fn destructive_actions_are_confirmed_by_osd() {
        for action in [
            PowerAction::Restart,
            PowerAction::ShutDown,
            PowerAction::LogOut,
        ] {
            assert!(action.osd_confirmation_arg().is_some(), "{action:?}");
        }
        for action in [PowerAction::Sleep, PowerAction::Lock] {
            assert!(action.osd_confirmation_arg().is_none(), "{action:?}");
        }
    }
}
