use futures_util::future::{self};
use std::net::IpAddr;
use std::sync::Arc;
use thiserror::Error;
use thrussh::client::Handle;
use thrussh::*;
use thrussh_keys::key::PublicKey;

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
    fn check_server_key(self, server_public_key: &PublicKey) -> Self::FutureBool {
        println!("check_server_key: {:?}", server_public_key);
        self.finished_bool(true)
    }
    fn channel_open_confirmation(
        self,
        channel: ChannelId,
        _max_packet_size: u32,
        _window_size: u32,
        session: client::Session,
    ) -> Self::FutureUnit {
        println!("channel_open_confirmation: {:?}", channel);
        self.finished(session)
    }
    fn data(self, channel: ChannelId, data: &[u8], session: client::Session) -> Self::FutureUnit {
        println!(
            "data on channel {:?}: {:?}",
            channel,
            std::str::from_utf8(data)
        );
        self.finished(session)
    }
}

pub struct SshSession {
    handle: Handle<Client>,
}

impl SshSession {
    pub async fn open(ip: IpAddr, password: &str) -> Result<Self, SshError> {
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

    pub async fn exec<S: Into<String>>(&mut self, cmd: S) -> Result<(), SshError> {
        let mut channel = self.handle.channel_open_session().await?;
        println!("exec");
        channel.exec(true, cmd).await?;
        println!("exec'd");
        if let Some(msg) = channel.wait().await {
            println!("{:?}", msg)
        }
        Ok(())
    }
}
