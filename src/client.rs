use crate::connection::{connections, ReaderCommand, SubscribeMessage, WriteSender, ReadSender};
use crate::connection_config::ConnectionConfig;
use crate::frame::Frame;
use crate::header;
use crate::subscription::{AckMode, AckOrNack, Subscription};
use std::sync::Arc;
use std::{fmt, io};
use tokio::net::tcp::ReuniteError;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver};
use tokio::sync::Mutex;
use tracing::{debug, info};

// const GRACE_PERIOD_MULTIPLIER: f32 = 2.0;

pub struct Client {
    config: ConnectionConfig,
    pub(crate) state: Arc<Mutex<SessionState>>,
    response_receiver: Option<Receiver<SessionEvent>>,
    pub(crate) read_cmd_sender: Option<ReadSender>,
    write_cmd_sender: Option<WriteSender>,
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
        OutstandingReceipt { original_frame }
    }
}

pub struct SessionState {
    #[allow(unused)]
    next_transaction_id: u32,
    next_subscription_id: u32,
    next_receipt_id: u32,
    pub rx_heartbeat_ms: Option<u32>,
    pub tx_heartbeat_ms: Option<u32>,
    // pub rx_heartbeat_timeout: Option<Timeout>,
    // pub tx_heartbeat_timeout: Option<Timeout>,
    // pub subscriptions: HashMap<String, Subscription>,
    // pub outstanding_receipts: HashMap<String, OutstandingReceipt>,
}

impl SessionState {
    pub fn new() -> SessionState {
        SessionState {
            next_transaction_id: 0,
            next_subscription_id: 0,
            next_receipt_id: 0,
            rx_heartbeat_ms: None,
            // rx_heartbeat_timeout: None,
            tx_heartbeat_ms: None,
            // tx_heartbeat_timeout: None,
            // subscriptions: HashMap::new(),
            // outstanding_receipts: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    UnknownEvent(SessionEvent),
    ReuniteError(ReuniteError),
    ChannelError(String),
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
            Error::ChannelError(e) => write!(f, "{}", e),
        }
    }
}

impl Client {
    pub(crate) fn new(stream: TcpStream, config: ConnectionConfig) -> Self {
        debug!("connections starting");
        let (sender, receiver, write_sender) = connections(stream);
        debug!("connections started");
        // start reader from stream
        Client {
            config,
            state: Arc::new(Mutex::new(SessionState::new())),
            response_receiver: Some(receiver),
            read_cmd_sender: Some(sender),
            write_cmd_sender: Some(write_sender),
        }
    }

    pub async fn send(&self, fr: Frame) -> crate::Result<()> {
        self.write_cmd_sender.as_ref().unwrap().send_frame(fr).await
    }

    pub async fn connect(&mut self) -> crate::Result<()> {
        let _ = self.send(Frame::connect(1000, 1000)).await?;

        let receiver = self.response_receiver.as_mut().unwrap();
        let res = receiver.recv().await;

        match res {
            Some(SessionEvent::Connected) => Ok(()),
            _ => {
                //TODO: meaningful error
                Err(Error::StdIOError(io::Error::from(
                    io::ErrorKind::ConnectionAborted,
                )))
            }
        }
    }

    pub async fn acknowledge_frame(&self, frame: &Frame, which: AckOrNack) -> crate::Result<()> {
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
        // let _ = self.kill_reader().await;
        self.send(Frame::disconnect()).await
    }

    pub async fn reconnect(&mut self) -> crate::Result<()> {
        use std::net::ToSocketAddrs;

        // TODO: send shutdown signal to writer or reader

        let _ = self.write_cmd_sender.as_ref().unwrap().shutdown().await;
        let _ = self.read_cmd_sender.as_ref().unwrap().shutdown().await;

        info!("Reconnecting...");

        let address = (&self.config.host as &str, self.config.port)
            .to_socket_addrs()?
            .nth(0)
            .ok_or(io::Error::new(
                io::ErrorKind::Other,
                "address provided resolved to nothing",
            ))?;

        let stream = TcpStream::connect(&address).await?;
        let (sender, receiver, write_sender) = connections(stream);

        self.response_receiver = Some(receiver);
        self.read_cmd_sender = Some(sender);
        self.write_cmd_sender = Some(write_sender);

        // {
        //     let mut old_r = self.connection.read.lock().await;
        //     let mut old_w = self.connection.write.lock().await;
        //     let old_w = mem::replace(&mut *old_w, w);
        //     let old_r = mem::replace(&mut *old_r, r);
        //
        //     if let Ok(mut old_stream) = old_w
        //         .writer
        //         .reunite(old_r.reader)
        //         .map_err::<Error, _>(|e| e.into())
        //     {
        //         old_stream.shutdown().await?
        //     }
        // }

        self.connect().await
    }

    fn _on_message(&mut self, _frame: Frame) {}

    #[allow(unused)]
    async fn generate_transaction_id(&mut self) -> u32 {
        let mut state = self.state.lock().await;
        let id = state.next_transaction_id;
        state.next_transaction_id += 1;
        id
    }

    pub async fn generate_subscription_id(&mut self) -> u32 {
        let mut state = self.state.lock().await;
        let id = state.next_subscription_id;
        state.next_subscription_id += 1;
        id
    }

    pub async fn generate_receipt_id(&mut self) -> u32 {
        let mut state = self.state.lock().await;
        let id = state.next_receipt_id;
        state.next_receipt_id += 1;
        id
    }
    pub async fn subscribe(&mut self, mut subscription: Subscription) -> crate::Result<String> {
        let next_id = self.generate_subscription_id().await;

        let sub_id = format!("stomp-rs/{}", next_id);

        let mut subscribe_frame = Frame::subscribe(
            &sub_id.clone(),
            &subscription.destination,
            subscription.ack_mode,
        );

        subscribe_frame
            .headers
            .concat(&mut subscription.headers.clone());

        self.send(subscribe_frame).await?;
        debug!(
            "Registering callback for subscription id '{}' from builder",
            sub_id
        );

        // TODO buffer size
        let (tx, mut rx) = mpsc::channel::<Frame>(100);

        let sender = self.read_cmd_sender.as_mut();
        sender
            .unwrap()
            .send(ReaderCommand::Subscribe(SubscribeMessage {
                id: sub_id.clone(),
                destination: subscription.destination.clone(),
                resp: tx,
            }))
            .await?;

        let sender = self.write_cmd_sender.as_ref().unwrap().clone();

        tokio::task::spawn(async move {
            while let Some(frame) = rx.recv().await {
                let ack = frame.headers.get_ack();
                let callback_result = subscription.handler.on_message(&frame);
                debug!("Executing.");
                match subscription.ack_mode {
                    AckMode::Auto => {
                        debug!("Auto ack, no frame sent.");
                    }
                    AckMode::Client | AckMode::ClientIndividual => {
                        if let Some(ack) = ack {
                            let _ = sender
                                .send_frame(match callback_result {
                                    AckOrNack::Ack => Frame::ack(ack.0),
                                    _ => Frame::nack(ack.0),
                                })
                                .await;
                        }
                    }
                }
            }
        });

        Ok(sub_id.to_string())
    }
}

mod test {
    use crate::client_builder;
    use crate::connection_config::HeartBeat;
    use crate::frame::Frame;
    use crate::header::HeaderList;
    use crate::subscription::AckMode;
    use crate::subscription::AckOrNack::Ack;
    use crate::subscription_builder::SubscriptionBuilder;
    use tokio::time::Duration;
    use tracing::debug;
    use tracing_subscriber;

    #[tokio::test]
    async fn test_connect() {
        let _ = tracing_subscriber::fmt::try_init();
        debug!("starting");
        let mut client = client_builder::ClientBuilder::new("localhost", 6379)
            .with(HeartBeat(500, 300))
            .start()
            .await
            .unwrap();
        let con_res = client.connect().await;
        assert!(!con_res.is_err(), "should be no error connecting");

        let sb = SubscriptionBuilder {
            session: &mut client,
            destination: "test",
            ack_mode: AckMode::Client,
            handler: Box::new(|f: &Frame| {
                debug!("HEEEEEEY {}", f.command);
                Ack
            }),
            headers: HeaderList::new(),
        };
        let _ = sb.start().await;
        debug!("started");

        let send_res = client.send(Frame::send("test", "test".as_ref())).await;
        assert!(!send_res.is_err(), "should be no error sending frame");

        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = client.reconnect().await;
    }
}
