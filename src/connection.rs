use crate::frame::{Transmission};
use crate::parsing::{decode, encode};
use bytes::BytesMut;
use std::io::{self};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};


pub fn connections(socket: TcpStream) -> (ReadConnection, WriteConnection) {
    let (r, w) = socket.into_split();
    (
        ReadConnection {
            reader: r,
            buffer: BytesMut::with_capacity(4 * 1024),
        },
        WriteConnection {
            writer: w,
            buffer: BytesMut::with_capacity(4 * 1024),
        }
    )
}

/// Send `Transmission` values to a remote peer.
#[derive(Debug)]
pub struct WriteConnection {
    pub(crate) writer: OwnedWriteHalf,
    buffer: BytesMut,
}
impl WriteConnection {
    pub async fn write_transmission(&mut self, trans: Transmission) -> io::Result<()> {
        let mut out_buf = BytesMut::new();

        encode(trans, &mut out_buf)?;

        self.writer.write(&mut out_buf).await?;

        // Ensure the encoded frame is written to the socket. The calls above
        // are to the buffered stream and writes. Calling `flush` writes the
        // remaining contents of the buffer to the socket.
        self.writer.flush().await
    }
}

/// Receive `Transmission` values from a remote peer.
#[derive(Debug)]
pub struct ReadConnection {
    pub(crate) reader: OwnedReadHalf,
    buffer: BytesMut,
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
    pub async fn read_transmission(&mut self) -> crate::Result<Option<Transmission>> {
        loop {
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
                    return Err(crate::client::Error::StdIOError(io::ErrorKind::ConnectionReset.into()));
                }
            }
        }
    }

    fn parse_transmission(&mut self) -> crate::Result<Option<Transmission>> {
        match decode(&mut self.buffer) {
            Ok(Some(t)) => Ok(Some(t)),
            Ok(None) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

}
