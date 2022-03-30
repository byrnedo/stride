use crate::client_builder::ClientBuilder;
use crate::header::{Header, SuppressedHeader};
// use crate::message_builder::MessageBuilder;
use crate::connection_config::{Credentials, HeartBeat, OwnedCredentials};
use crate::subscription::AckMode;
use crate::subscription_builder::SubscriptionBuilder;

pub trait OptionSetter<T> {
    fn set_option(self, builder: T) -> T;
}

// impl <'a> OptionSetter<MessageBuilder<'a>> for Header {
//     fn set_option(self, mut builder: MessageBuilder<'a>) -> MessageBuilder<'a> {
//         builder.frame.headers.push(self);
//         builder
//     }
// }
//
// impl <'a, 'b> OptionSetter<MessageBuilder<'b>> for SuppressedHeader<'a> {
//     fn set_option(self, mut builder: MessageBuilder<'b>) -> MessageBuilder<'b> {
//         let SuppressedHeader(key) = self;
//         builder.frame.headers.retain(|header| (*header).get_key() != key);
//         builder
//     }
// }
//
// impl <'a, 'b> OptionSetter<MessageBuilder<'b>> for ContentType<'a> {
//     fn set_option(self, mut builder: MessageBuilder<'b>) -> MessageBuilder<'b> {
//         let ContentType(content_type) = self;
//         builder.frame.headers.push(Header::new("content-type", content_type));
//         builder
//     }
// }

impl OptionSetter<ClientBuilder> for Header {
    fn set_option(self, mut builder: ClientBuilder) -> ClientBuilder {
        builder.config.headers.push(self);
        builder
    }
}

impl OptionSetter<ClientBuilder> for HeartBeat {
    fn set_option(self, mut builder: ClientBuilder) -> ClientBuilder {
        builder.config.heartbeat = self;
        builder
    }
}

impl<'b> OptionSetter<ClientBuilder> for Credentials<'b> {
    fn set_option(self, mut builder: ClientBuilder) -> ClientBuilder {
        builder.config.credentials = Some(OwnedCredentials::from(self));
        builder
    }
}

impl<'b> OptionSetter<ClientBuilder> for SuppressedHeader<'b> {
    fn set_option(self, mut builder: ClientBuilder) -> ClientBuilder {
        let SuppressedHeader(key) = self;
        builder
            .config
            .headers
            .retain(|header| (*header).get_key() != key);
        builder
    }
}

impl<'a> OptionSetter<SubscriptionBuilder<'a>> for Header {
    fn set_option(self, mut builder: SubscriptionBuilder<'a>) -> SubscriptionBuilder<'a> {
        builder.headers.push(self);
        builder
    }
}

impl<'a, 'b> OptionSetter<SubscriptionBuilder<'b>> for SuppressedHeader<'a> {
    fn set_option(self, mut builder: SubscriptionBuilder<'b>) -> SubscriptionBuilder<'b> {
        let SuppressedHeader(key) = self;
        builder.headers.retain(|header| (*header).get_key() != key);
        builder
    }
}

impl<'a> OptionSetter<SubscriptionBuilder<'a>> for AckMode {
    fn set_option(self, mut builder: SubscriptionBuilder<'a>) -> SubscriptionBuilder<'a> {
        builder.ack_mode = self;
        builder
    }
}
//
// impl <'a> OptionSetter<MessageBuilder<'a>> for GenerateReceipt {
//     fn set_option(self, mut builder: MessageBuilder<'a>) -> MessageBuilder<'a> {
//         let next_id = builder.session.generate_receipt_id();
//         let receipt_id = format!("message/{}", next_id);
//         builder.receipt_request = Some(ReceiptRequest::new(receipt_id.clone()));
//         builder.frame.headers.push(Header::new("receipt", receipt_id.as_ref()));
//         builder
//     }
// }
//
// impl <'a> OptionSetter<SubscriptionBuilder<'a>> for GenerateReceipt {
//     fn set_option(self, mut builder: SubscriptionBuilder<'a>) -> SubscriptionBuilder<'a> {
//         let next_id = builder.session.generate_receipt_id();
//         let receipt_id = format!("message/{}", next_id);
//         builder.receipt_request = Some(ReceiptRequest::new(receipt_id.clone()));
//         builder.headers.push(Header::new("receipt", receipt_id.as_ref()));
//         builder
//     }
// }
