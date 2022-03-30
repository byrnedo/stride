use crate::connection::{ReadConnection, WriteConnection, connections};
use crate::frame::{Frame, Command};
use crate::frame::Transmission::{HeartBeat, CompleteFrame};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::{Arc};
use crate::subscription::{AckMode, Subscription, AckOrNack};
use std::collections::HashMap;
use crate::{header, connection_config};
use tracing::{debug, info, error};
use crate::connection_config::ConnectionConfig;
use tokio::io::{AsyncWriteExt};
use std::{mem, fmt, io};
use tokio::task;
use crate::client::Error::UnknownEvent;
use tokio::net::tcp::ReuniteError;
use tokio::sync::oneshot;

const GRACE_PERIOD_MULTIPLIER: f32 = 2.0;


struct Connection {
    read: Mutex<ReadConnection>,
    write: Mutex<WriteConnection>,
}


#[derive(Clone)]
pub struct Client {
    config: ConnectionConfig,
    connection: Arc<Connection>,
    pub(crate) events: Arc<Mutex<Vec<SessionEvent>>>,
    pub(crate) state: Arc<SessionState>,
    kill_reader_chan: Arc<Mutex<Option<oneshot::Sender<bool>>>>,
}

#[derive(Debug)]
pub enum SessionEvent {
    Connected,
    ErrorFrame(Frame),
    Receipt {
        id: String,
        original: Frame,
        receipt: Frame,
    },
    Message {
        destination: String,
        ack_mode: AckMode,
        frame: Frame,
    },
    SubscriptionlessFrame(Frame),
    UnknownFrame(Frame),
    Disconnected(DisconnectionReason),
}

impl fmt::Display for SessionEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SessionEvent::Connected => write!(f, "CONNECTED"),
            SessionEvent::ErrorFrame(_) => write!(f, "ERROR"),
            SessionEvent::Receipt { .. } => write!(f, "RECEIPT"),
            SessionEvent::Message { .. } => write!(f, "MESSAGE"),
            SessionEvent::SubscriptionlessFrame(_) => write!(f, "SUBSCRIPTION-LESS-FRAME"),
            SessionEvent::UnknownFrame(_) => write!(f, "UNKNOWN-FRAME"),
            SessionEvent::Disconnected(_) => write!(f, "DISCONNECTED"),
        }
    }
}


#[derive(Debug)]
pub enum DisconnectionReason {
    RecvFailed(::std::io::Error),
    ConnectFailed(::std::io::Error),
    SendFailed(::std::io::Error),
    ClosedByOtherSide,
    HeartbeatTimeout,
    Requested,
}

pub struct OutstandingReceipt {
    pub original_frame: Frame,
}

impl OutstandingReceipt {
    pub fn new(original_frame: Frame) -> Self {
        OutstandingReceipt {
            original_frame
        }
    }
}

pub struct SessionState {
    // next_transaction_id: u32,
    // next_subscription_id: u32,
    // next_receipt_id: u32,
    pub rx_heartbeat_ms: Mutex<Option<u32>>,
    pub tx_heartbeat_ms: Mutex<Option<u32>>,
    // pub rx_heartbeat_timeout: Option<Timeout>,
    // pub tx_heartbeat_timeout: Option<Timeout>,
    pub subscriptions: Mutex<HashMap<String, Subscription>>,
    pub outstanding_receipts: HashMap<String, OutstandingReceipt>,
}

impl SessionState {
    pub fn new() -> SessionState {
        SessionState {
            // next_transaction_id: 0,
            // next_subscription_id: 0,
            // next_receipt_id: 0,
            rx_heartbeat_ms: Mutex::new(None),
            // rx_heartbeat_timeout: None,
            tx_heartbeat_ms: Mutex::new(None),
            // tx_heartbeat_timeout: None,
            subscriptions: Mutex::new(HashMap::new()),
            outstanding_receipts: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    UnknownEvent(SessionEvent),
    ReuniteError(ReuniteError),
    StdIOError(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::StdIOError(e)
    }
}

impl From<ReuniteError> for Error {
    fn from(e: ReuniteError) -> Self {
        Error::ReuniteError(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::UnknownEvent(e) => write!(f, "unknown event {}", e),
            Error::StdIOError(e) => write!(f, "{}", e),
            Error::ReuniteError(e) => write!(f, "{}", e),
        }
    }
}

impl Client {
    pub(crate) fn new(stream: TcpStream, config: ConnectionConfig) -> Self {
        let (r, w) = connections(stream);
        // start reader from stream
        let client = Client {
            config,
            connection: Arc::new(Connection { read: Mutex::new(r), write: Mutex::new(w) }),
            events: Arc::new(Mutex::new(vec!())),
            state: Arc::new(SessionState::new()),
            kill_reader_chan: Arc::new(Mutex::new(None)),
        };
        // let subs = client.state.subscriptions.clone();
        // let events = client.events.clone();
        //
        // let c_clone = client.connection.clone();


        client
    }


    pub async fn send(&self, fr: Frame) -> crate::Result<()> {
        let mut c = self.connection.write.lock().await;
        c.write_transmission(CompleteFrame(fr)).await.map_err(|e| e.into())
    }

    pub async fn connect(&mut self) -> crate::Result<()> {
        let _ = self.kill_reader().await;
        let mut c = self.connection.write.lock().await;
        let _ = c.write_transmission(CompleteFrame(Frame::connect(1000, 1000))).await?;
        drop(c);
        let res = self.read_one_and_handle().await.map_err::<Error, _>(|e| e.into())?;

        match res {
            Some(SessionEvent::Connected) => {
                let (tx, rx) = oneshot::channel::<bool>();
                self.kill_reader_chan = Arc::new(Mutex::new(Some(tx)));
                let client = self.clone();
                task::spawn(async move {
                    tokio::select! {
                        _ = client.read_loop() => {},
                        _ = rx => {
                            debug!("reader killed")
                        },
                    }
                });
                Ok(())
            }
            Some(rest) => Err(UnknownEvent(rest)),
            None => Ok(())
        }
    }

    pub async fn acknowledge_frame(&mut self, frame: &Frame, which: AckOrNack) -> crate::Result<()> {
        if let Some(header::Ack(ack_id)) = frame.headers.get_ack() {
            let ack_frame = if let AckOrNack::Ack = which {
                Frame::ack(ack_id)
            } else {
                Frame::nack(ack_id)
            };
            return self.send(ack_frame).await;
        }
        Ok(())
    }
    pub async fn disconnect(&mut self) -> crate::Result<()> {
        let _ = self.kill_reader().await;
        self.send(Frame::disconnect()).await
    }
    pub async fn kill_reader(&self) -> Result<(), bool> {
        let mut tx = self.kill_reader_chan.lock().await;

        if let Some(tx) = mem::replace(&mut *tx, None) {
            return tx.send(true);
        }
        Ok(())
    }

    pub async fn reconnect(&mut self) -> crate::Result<()> {
        use std::net::ToSocketAddrs;

        info!("Reconnecting...");

        let address = (&self.config.host as &str, self.config.port)
            .to_socket_addrs()?.nth(0)
            .ok_or(io::Error::new(io::ErrorKind::Other, "address provided resolved to nothing"))?;


        let stream = TcpStream::connect(&address).await?;
        let (r, w) = connections(stream);

        {
            let mut old_r = self.connection.read.lock().await;
            let mut old_w = self.connection.write.lock().await;
            let old_w = mem::replace(&mut *old_w, w);
            let old_r = mem::replace(&mut *old_r, r);

            if let Ok(mut old_stream) = old_w.writer.reunite(old_r.reader).map_err::<Error, _>(|e| e.into()) {
                old_stream.shutdown().await?
            }
        }

        self.connect().await
    }

    fn _on_message(&mut self, _frame: Frame) {}

    async fn read_one_and_handle(&self) -> crate::Result<Option<SessionEvent>> {
        let mut r = self.connection.read.lock().await;
        match r.read_transmission().await {
            Ok(Some(CompleteFrame(frame))) => {
                debug!("Received frame: {:?}", frame);
                match frame.command {
                    Command::Error => {
                        Ok(Some(SessionEvent::ErrorFrame(frame)))
                    }
                    // Command::Receipt => self.handle_receipt(frame),
                    Command::Connected => Ok(Some(self.clone().on_connected_frame_received(frame).await?)),
                    Command::Message => Ok(Some(self.clone().on_message(frame).await?)),
                    _ => {
                        Ok(Some(SessionEvent::UnknownFrame(frame)))
                    }
                }
            }
            Ok(Some(HeartBeat)) => {
                debug!("Received heartbeat");
                let mut w = self.connection.write.lock().await;
                let _ = w.write_transmission(HeartBeat).await;
                Ok(None)
            }
            Ok(None) => {
                debug!("Received nothing");
                Ok(None)
            }

            Err(err) => {
                error!("Received error: {}", err);
                Err(err)
            }
        }
    }

    async fn read_loop(self) -> crate::Result<()> {
        while let res = self.read_one_and_handle().await {
            match res {
                Ok(Some(evt)) => {
                    let mut events = self.events.lock().await;
                    events.push(evt);
                }
                Ok(None) => {}
                Err(err) => return Err(err.into())
            }
        }
        Ok(())
    }

    async fn on_message(self, frame: Frame) -> crate::Result<SessionEvent> {
        debug!("command: {}", frame.command);
        debug!("{:?}", frame);
        let mut sub_data = None;

        if let Some(header::Subscription(sub_id)) = frame.headers.get_subscription() {
            let subs = self.state.subscriptions.lock().await;
            if let Some(ref sub) = subs.get(sub_id) {
                sub_data = Some((sub.destination.clone(), sub.ack_mode));
            }
        }

        if let Some((destination, ack_mode)) = sub_data {
            Ok(SessionEvent::Message {
                destination,
                ack_mode,
                frame,
            })
        } else {
            debug!("subless frame");
            Ok(SessionEvent::SubscriptionlessFrame(frame))
        }
    }

    async fn on_connected_frame_received(self, connected_frame: Frame) -> crate::Result<SessionEvent> {
        // The Client's requested tx/rx HeartBeat timeouts
        let connection_config::HeartBeat(client_tx_ms, client_rx_ms) = self.config.heartbeat;

        // The timeouts the server is willing to provide
        let (server_tx_ms, server_rx_ms) = match connected_frame.headers.get_heart_beat() {
            Some(header::HeartBeat(tx_ms, rx_ms)) => (tx_ms, rx_ms),
            None => (0, 0),
        };

        let (agreed_upon_tx_ms, agreed_upon_rx_ms) = ConnectionConfig::select_heartbeat(client_tx_ms,
                                                                                        client_rx_ms,
                                                                                        server_tx_ms,
                                                                                        server_rx_ms);
        let mut rxh = self.state.rx_heartbeat_ms.lock().await;
        *rxh = Some((agreed_upon_rx_ms as f32 * GRACE_PERIOD_MULTIPLIER) as u32);

        let mut txh = self.state.tx_heartbeat_ms.lock().await;
        *txh = Some((agreed_upon_tx_ms as f32 * GRACE_PERIOD_MULTIPLIER) as u32);


        // self.register_tx_heartbeat_timeout()?;
        // self.register_rx_heartbeat_timeout()?;

        Ok(SessionEvent::Connected)
    }
}

mod test {
    use crate::client_builder;
    use crate::frame::Frame;
    use tokio::time::Duration;
    use tracing::{debug};
    use tracing_subscriber;
    use crate::connection_config::HeartBeat;
    use crate::subscription::AckMode;

    #[tokio::test]
    async fn test_connect() {
        let _ = tracing_subscriber::fmt::try_init();
        let mut client = client_builder::ClientBuilder::new("localhost", 6379)
            .with(HeartBeat(500, 300))
            .start().await.unwrap();
        debug!("{:?} {:?}", client.state.rx_heartbeat_ms, client.state.tx_heartbeat_ms);
        let con_res = client.connect().await;
        assert!(!con_res.is_err(), "should be no error connecting");

        debug!("{:?} {:?}", client.state.rx_heartbeat_ms, client.state.tx_heartbeat_ms);

        let send_res = client.send(Frame::subscribe("1", "test", AckMode::Client)).await;
        assert!(!send_res.is_err(), "should be no error sending frame");

        let send_res = client.send(Frame::send("test", "test".as_ref())).await;
        assert!(!send_res.is_err(), "should be no error sending frame");

        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = client.reconnect().await;
    }
}
