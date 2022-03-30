use crate::client::Client;
use crate::header::HeaderList;
use crate::option_setter::OptionSetter;
use crate::subscription::{AckMode, MessageHandler, Subscription};

pub struct SubscriptionBuilder<'a> {
    pub session: &'a mut Client,
    pub destination: &'a str,
    pub ack_mode: AckMode,
    pub handler: Box<dyn MessageHandler + Send>,
    pub headers: HeaderList,
}

impl<'a> SubscriptionBuilder<'a> {
    #[allow(dead_code)]
    pub async fn start(self) -> crate::Result<String> {
        let subscription = Subscription::new(
            self.destination,
            self.ack_mode,
            self.headers.clone(),
            self.handler,
        );
        self.session.subscribe(subscription).await
    }

    #[allow(dead_code)]
    pub fn with<T>(self, option_setter: T) -> SubscriptionBuilder<'a>
    where
        T: OptionSetter<SubscriptionBuilder<'a>>,
    {
        option_setter.set_option(self)
    }
}
