use futures_util::future::{self};
use std::io::Write;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use thrussh::client::Handle;
use thrussh::*;
use thrussh_keys::key::PublicKey;
use tokio::time::{sleep, timeout};

struct Client {}

#[derive(Debug, Error)]
pub enum SshError {
    #[error(transparent)]
    Other(#[from] SshErrorImpl),
    #[error("Invalid credentials")]
    Unauthorized,
    #[error("Connection timeout")]
    ConnectionTimeout,
    #[error("Disconnected by server")]
    Disconnected,
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct SshErrorImpl(thrussh::Error);

impl From<thrussh::Error> for SshError {
    fn from(e: thrussh::Error) -> Self {
        match e {
            thrussh::Error::Disconnect => SshError::Disconnected,
            thrussh::Error::HUP => SshError::Disconnected,
            thrussh::Error::ConnectionTimeout => SshError::ConnectionTimeout,
            thrussh::Error::IO(io) if io.raw_os_error() == Some(110) => SshError::ConnectionTimeout,
            e => SshError::Other(SshErrorImpl(e)),
        }
    }
}

impl client::Handler for Client {
    type Error = SshError;
    type FutureBool = future::Ready<Result<(Self, bool), SshError>>;
    type FutureUnit = future::Ready<Result<(Self, client::Session), SshError>>;

    fn finished_bool(self, b: bool) -> Self::FutureBool {
        future::ready(Ok((self, b)))
    }
    fn finished(self, session: client::Session) -> Self::FutureUnit {
        future::ready(Ok((self, session)))
    }
    fn check_server_key(self, _server_public_key: &PublicKey) -> Self::FutureBool {
        self.finished_bool(true)
    }
}

pub struct SshSession {
    handle: Handle<Client>,
}

impl SshSession {
    pub async fn open(ip: IpAddr, password: &str) -> Result<Self, SshError> {
        Ok(timeout(Duration::from_secs(5 * 60), async move {
            loop {
                sleep(Duration::from_secs(1)).await;
                match SshSession::open_impl(ip, password).await {
                    Ok(ssh) => return Ok(ssh),
                    Err(SshError::ConnectionTimeout) => {}
                    Err(e) => return Err(e),
                }
            }
        })
        .await
        .map_err(|_| SshError::ConnectionTimeout)??)
    }

    async fn open_impl(ip: IpAddr, password: &str) -> Result<Self, SshError> {
        let config = thrussh::client::Config::default();
        let config = Arc::new(config);
        let sh = Client {};

        let mut handle = thrussh::client::connect(config, (ip, 22), sh).await?;
        if handle.authenticate_password("root", password).await? {
            Ok(SshSession { handle })
        } else {
            Err(SshError::Unauthorized)
        }
    }

    pub async fn exec<S: Into<String>>(&mut self, cmd: S) -> Result<CommandResult, SshError> {
        let mut channel = self.handle.channel_open_session().await?;
        channel.exec(true, cmd).await?;
        let mut output = Vec::new();
        let mut code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                thrussh::ChannelMsg::Data { ref data } => {
                    output.write_all(data).unwrap();
                }
                thrussh::ChannelMsg::ExitStatus { exit_status } => {
                    code = Some(exit_status);
                }
                _ => {}
            }
        }
        Ok(CommandResult { output, code })
    }

    pub async fn close(mut self) -> Result<(), SshError> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;
        self.handle.await?;
        Ok(())
    }
}

pub struct CommandResult {
    output: Vec<u8>,
    pub code: Option<u32>,
}

impl CommandResult {
    pub fn output(&self) -> String {
        String::from_utf8_lossy(&self.output).into()
    }

    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}
