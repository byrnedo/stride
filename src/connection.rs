use crate::client::Error::StdIOError;
use crate::client::{Error, SessionEvent};
use crate::frame::Transmission;
use crate::frame::Transmission::CompleteFrame;
use crate::frame::{Command, Frame};
use crate::header;
use crate::parsing::{decode, encode};
use bytes::BytesMut;
use std::collections::HashMap;
use std::io::{self};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error};

pub(crate) fn connections(socket: TcpStream) -> (ReadSender, Receiver<SessionEvent>, WriteSender) {
    let (read_cmd_tx, read_cmd_rx) = mpsc::channel::<ReaderCommand>(100);
    let (read_res_tx, read_res_rx) = mpsc::channel::<SessionEvent>(100);
    let (write_cmd_tx, write_cmd_rx) = mpsc::channel::<WriterCommand>(100);
    let write_cmd_tx2 = write_cmd_tx.clone();

    let (r_half, w_half) = socket.into_split();

    let rc = ReadConnection {
        reader: r_half,
        buffer: BytesMut::with_capacity(4 * 1024),
        subscriptions: HashMap::new(),
        response_sender: read_res_tx,
        write_cmd_sender: write_cmd_tx,
    };
    let wc = WriteConnection {
        writer: w_half,
        buffer: BytesMut::with_capacity(4 * 1024),
        receiver: write_cmd_rx,
        read_cmd_sender: read_cmd_tx.clone(),
    };
    tokio::task::spawn(rc.work_loop(read_cmd_rx));
    tokio::task::spawn(wc.work_loop());
    (
        ReadSender(read_cmd_tx),
        read_res_rx,
        WriteSender(write_cmd_tx2),
    )
}

pub struct WriteTransmission {
    pub trans: Transmission,
    pub resp: oneshot::Sender<crate::Result<()>>,
}

pub enum WriterCommand {
    Write(WriteTransmission),
    Shutdown(),
}

/// Send `Transmission` values to a remote peer.
#[derive(Debug)]
pub struct WriteConnection {
    pub(crate) writer: OwnedWriteHalf,
    buffer: BytesMut,
    receiver: Receiver<WriterCommand>,
    read_cmd_sender: Sender<ReaderCommand>,
}

impl WriteConnection {
    pub(crate) async fn work_loop(mut self) -> crate::Result<()> {
        loop {
            tokio::select! {
                Some(write_cmd) = self.receiver.recv() => {
                    match write_cmd {
                        WriterCommand::Write(WriteTransmission{trans, resp})=> {
                           let result = self.write_transmission(trans).await.map_err(|e| StdIOError(e));
                           let _ = resp.send(result);
                        },
                        WriterCommand::Shutdown() => {
                            debug!("shutting down writer work loop");
                            return Ok(())
                        }
                    }
                }
            }
        }
    }

    async fn write_transmission(&mut self, trans: Transmission) -> io::Result<()> {
        let mut out_buf = BytesMut::new();

        encode(trans, &mut out_buf)?;

        self.writer.write(&mut out_buf).await?;

        // Ensure the encoded frame is written to the socket. The calls above
        // are to the buffered stream and writes. Calling `flush` writes the
        // remaining contents of the buffer to the socket.
        self.writer.flush().await
    }
}

#[derive(Debug)]
pub struct SubscribeMessage {
    pub id: String,
    pub destination: String,
    pub resp: mpsc::Sender<Frame>,
}

pub enum ReaderCommand {
    Subscribe(SubscribeMessage),
    Shutdown(),
}

/// Receive `Transmission` values from a remote peer.
#[derive(Debug)]
pub struct ReadConnection {
    pub(crate) reader: OwnedReadHalf,
    buffer: BytesMut,
    subscriptions: HashMap<String, SubscribeMessage>,
    response_sender: Sender<SessionEvent>,
    write_cmd_sender: Sender<WriterCommand>,
}

impl ReadConnection {
    /// Read a single `Transmission` value from the underlying stream.
    ///
    /// The function waits until it has retrieved enough data to parse a transmission.
    /// Any data remaining in the read buffer after the transmission has been parsed is
    /// kept there for the next call to `read_transmission`.
    ///
    /// # Returns
    ///
    /// On success, the received transmission is returned. If the `TcpStream`
    /// is closed in a way that doesn't break a transmission in half, it returns
    /// `None`. Otherwise, an error is returned.
    pub(crate) async fn work_loop(
        mut self,
        mut receiver: Receiver<ReaderCommand>,
    ) -> crate::Result<()> {
        loop {
            tokio::select! {
                Some(cmd) = receiver.recv() => {
                    match cmd {
                        ReaderCommand::Subscribe(sub) => {
                            let id = sub.id.clone();
                            self.subscriptions.insert(id, sub);
                        }
                        ReaderCommand::Shutdown() => {
                            debug!("shutting down reader work loop");
                            return Ok(())
                        }
                    }

                },
                res = self.read_and_parse() => {
                    match res {
                        Ok(Some(Transmission::CompleteFrame(frame))) => {
                            debug!("Received frame: {:?}", frame);
                            match frame.command {
                                Command::Error => {
                                    let _= self.response_sender.send(SessionEvent::ErrorFrame(frame)).await;
                                }
                                // Command::Receipt => self.handle_receipt(frame),
                                Command::Connected => {
                                    let _= self.response_sender.send(SessionEvent::Connected).await;
                                }
                                Command::Message => self.on_message(frame).await?,
                                _ => {}
                            }
                        }
                        Ok(Some(Transmission::HeartBeat)) => {
                            debug!("Received heartbeat");

                            let (tx, rx) = oneshot::channel::<crate::Result<()>>();
                            let _ = self.write_cmd_sender.send(WriterCommand::Write(WriteTransmission{trans: Transmission::HeartBeat, resp: tx})).await;
                            let _ = rx.await;
                        }
                        Ok(None) => {
                            // do nothing
                        }

                        Err(err) => {
                            error!("Received error: {}", err);
                        }
                    }
                }
            }
        }
    }

    async fn read_and_parse(&mut self) -> crate::Result<Option<Transmission>> {
        if let Some(t) = self.parse_transmission()? {
            return Ok(Some(t));
        }

        // There is not enough buffered data to read a frame. Attempt to
        // read more data from the socket.
        //
        // On success, the number of bytes is returned. `0` indicates "end
        // of stream".
        if 0 == self.reader.read_buf(&mut self.buffer).await? {
            // The remote closed the connection. For this to be a clean
            // shutdown, there should be no data in the read buffer. If
            // there is, this means that the peer closed the socket while
            // sending a frame.
            if self.buffer.is_empty() {
                return Ok(None);
            } else {
                return Err(crate::client::Error::StdIOError(
                    io::ErrorKind::ConnectionReset.into(),
                ));
            }
        }
        Ok(None)
    }

    fn parse_transmission(&mut self) -> crate::Result<Option<Transmission>> {
        match decode(&mut self.buffer) {
            Ok(Some(t)) => Ok(Some(t)),
            Ok(None) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn on_message(&self, frame: Frame) -> crate::Result<()> {
        debug!("command: {}", frame.command);
        debug!("{:?}", frame);
        {
            // This extra scope is required to free up `frame` and `self.subscriptions`
            // following a borrow.

            // Find the subscription ID on the frame that was received
            let header::Subscription(sub_id) = frame
                .headers
                .get_subscription()
                .expect("Frame did not contain a subscription header.");

            // Look up the appropriate Subscription object
            let subscription = self
                .subscriptions
                .get(sub_id)
                .expect("Received a message for an unknown subscription.");

            let resp = &subscription.resp;
            let _ = resp.send(frame).await;
        }

        Ok(())
    }
}

pub(crate) struct ReadSender(Sender<ReaderCommand>);

impl ReadSender {
    pub async fn shutdown(&self) -> crate::Result<()> {
        self.0
            .send(ReaderCommand::Shutdown())
            .await
            .map_err(|e| Error::ChannelError(e.to_string()))
    }
    pub async fn send(&self, cmd: ReaderCommand) -> crate::Result<()> {
        self.0
            .send(cmd)
            .await
            .map_err(|e| Error::ChannelError(e.to_string()))
    }
}

#[derive(Clone)]
pub(crate) struct WriteSender(Sender<WriterCommand>);

impl WriteSender {
    pub async fn shutdown(&self) -> crate::Result<()> {
        self.0
            .send(WriterCommand::Shutdown())
            .await
            .map_err(|e| Error::ChannelError(e.to_string()))
    }
    pub async fn send_frame(&self, fr: Frame) -> crate::Result<()> {
        let (tx, rx) = oneshot::channel::<crate::Result<()>>();
        let _ = self
            .0
            .send(WriterCommand::Write(WriteTransmission {
                trans: CompleteFrame(fr),
                resp: tx,
            }))
            .await;
        rx.await.map_err(|e| Error::ChannelError(e.to_string()))?
    }
}
