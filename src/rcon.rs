use crate::Error;
use rcon::Connection;
use std::fmt::Debug;
use tokio::net::{TcpStream, ToSocketAddrs};
use tracing::instrument;

pub struct Rcon(Connection<TcpStream>);

impl Rcon {
    #[instrument(skip(password))]
    pub async fn new<A: ToSocketAddrs + Debug>(host: A, password: &str) -> Result<Self, Error> {
        Ok(Rcon(Connection::builder().connect(host, password).await?))
    }

    #[instrument(skip(self))]
    pub async fn player_count(&mut self) -> Result<usize, Error> {
        let status = self.0.cmd("status").await?;
        let player_lines = status
            .lines()
            .filter(|line| line.starts_with('#'))
            .filter(|line| !line.contains("# userid"))
            .filter(|line| !line.contains(" BOT "));
        Ok(player_lines.count())
    }
}
