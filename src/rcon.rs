use crate::Error;
use rcon::Connection;
use tokio::net::ToSocketAddrs;

pub struct Rcon(Connection);

impl Rcon {
    pub async fn new<A: ToSocketAddrs>(host: A, password: &str) -> Result<Self, Error> {
        Ok(Rcon(Connection::builder().connect(host, password).await?))
    }

    pub async fn player_count(&mut self) -> Result<usize, Error> {
        let status = self.0.cmd("status").await?;
        let player_lines = status
            .lines()
            .filter(|line| line.starts_with('#'))
            .filter(|line| !line.contains(" BOT "));
        Ok(player_lines.count())
    }
}
