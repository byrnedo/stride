use crate::client::Client;
use crate::connection_config::{ConnectionConfig, HeartBeat};
use crate::header::HeaderList;
use crate::header_list;
use crate::option_setter::OptionSetter;
use std::io;
use std::net::ToSocketAddrs;
use tokio::net::TcpStream;

pub struct ClientBuilder {
    pub config: ConnectionConfig,
}

impl ClientBuilder {
    pub fn new(host: &str, port: u16) -> ClientBuilder {
        let config = ConnectionConfig {
            host: host.to_owned(),
            port,
            credentials: None,
            heartbeat: HeartBeat(0, 0),
            headers: header_list![
             "host" => host,
             "accept-version" => "1.2",
             "content-length" => "0"
            ],
        };
        ClientBuilder { config: config }
    }

    ///
    /// ```
    /// use server::client_builder;
    /// use server::frame::{Transmission, Frame};
    ///
    /// #[tokio::main]
    /// async fn main() -> server::Result<()> {
    ///     let builder = client_builder::ClientBuilder::new("localhost", 6379);
    ///     let mut client = builder.start().await.unwrap();
    ///     client.send(Transmission(Frame::connect(1000, 1000))).await
    /// }
    /// ```
    #[allow(dead_code)]
    pub async fn start<'b, 'c>(self) -> crate::Result<Client> {
        let address = (&self.config.host as &str, self.config.port)
            .to_socket_addrs()?
            .nth(0)
            .ok_or(io::Error::new(
                io::ErrorKind::Other,
                "address provided resolved to nothing",
            ))?;

        match TcpStream::connect(&address).await {
            Ok(s) => Ok(Client::new(s, self.config)),
            Err(err) => Err(crate::client::Error::StdIOError(err)),
        }
    }

    #[allow(dead_code)]
    pub fn with<'b, T>(self, option_setter: T) -> ClientBuilder
    where
        T: OptionSetter<ClientBuilder>,
    {
        option_setter.set_option(self)
    }
}
