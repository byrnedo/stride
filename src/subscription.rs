use crate::frame::Frame;
use crate::header::HeaderList;
use tracing::debug;

#[derive(Debug, Copy, Clone)]
pub enum AckMode {
    Auto,
    Client,
    ClientIndividual,
}

impl AckMode {
    pub fn as_text(&self) -> &'static str {
        match *self {
            AckMode::Auto => "auto",
            AckMode::Client => "client",
            AckMode::ClientIndividual => "client-individual",
        }
    }
}

#[derive(Clone, Copy)]
pub enum AckOrNack {
    Ack,
    Nack,
}

pub trait MessageHandler {
    fn on_message(&mut self, frame: &Frame) -> AckOrNack;
}

pub struct Subscription {
    pub destination: String,
    pub ack_mode: AckMode,
    pub headers: HeaderList,
    pub handler: Box<dyn MessageHandler + Send>,
}

impl Subscription {
    pub fn new(
        destination: &str,
        ack_mode: AckMode,
        headers: HeaderList,
        message_handler: Box<dyn MessageHandler + Send>,
    ) -> Subscription {
        Subscription {
            destination: destination.to_string(),
            ack_mode,
            headers,
            handler: message_handler,
        }
    }
}

pub trait ToMessageHandler<'a> {
    fn to_message_handler(self) -> Box<dyn MessageHandler + 'a>;
}

impl<'a, T: 'a> ToMessageHandler<'a> for T
where
    T: MessageHandler,
{
    fn to_message_handler(self) -> Box<dyn MessageHandler + 'a> {
        Box::new(self) as Box<dyn MessageHandler>
    }
}

impl<'a> ToMessageHandler<'a> for Box<dyn MessageHandler + 'a> {
    fn to_message_handler(self) -> Box<dyn MessageHandler + 'a> {
        self
    }
}
// Support for Sender<T> in subscriptions

// struct SenderMessageHandler {
//     sender: Sender<Frame>
// }
//
// impl MessageHandler for SenderMessageHandler {
//     async fn on_message(&mut self, frame: &Frame) -> AckOrNack {
//         debug!("Sending frame...");
//         match self.sender.send(frame.clone()).await {
//             Ok(_) => Ack,
//             Err(error) => {
//                 error!("Failed to send frame: {}", error);
//                 Nack
//             }
//         }
//     }
// }
//
// impl <'a> ToMessageHandler<'a> for Sender<Frame> {
//     fn to_message_handler(self) -> Box<dyn MessageHandler + 'a> {
//         Box::new(SenderMessageHandler{sender : self}) as Box<dyn MessageHandler>
//     }
// }

impl<F> MessageHandler for F
where
    F: FnMut(&Frame) -> AckOrNack,
{
    fn on_message(&mut self, frame: &Frame) -> AckOrNack {
        debug!("Passing frame to closure...");
        self(frame)
    }
}
