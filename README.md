# Stride

Stomp client for rust using tokio.

`stride` aims to provides a full [STOMP](http://stomp.github.io/stomp-specification-1.2.html) 1.2 client implementation for the [Rust programming language](http://www.rust-lang.org/). This allows programs written in Rust to interact with message queueing services like [ActiveMQ](http://activemq.apache.org/), [RabbitMQ](http://www.rabbitmq.com/), [HornetQ](http://hornetq.jboss.org/) and [OpenMQ](https://mq.java.net/).

`stride` is based heavily on the invactive [stomp-rs](https://github.com/zslayton/stomp-rs)

- [x] Connect
- [x] Subscribe
- [x] Send
- [x] Acknowledge (Auto/Client/ClientIndividual)
- [ ] Transactions
- [x] Receipts
- [x] Disconnect
- [x] Heartbeats

## Examples
### Connect / Subscribe / Send
```rust
// TODO
```

### Session Configuration
```rust
// TODO
```

### Message Configuration
```rust
// TODO
```

### Subscription Configuration
```rust
// TODO
```

### Transactions
```rust
// TODO
```

### Handling RECEIPT frames
If you include a ReceiptHandler in your message, the client will request that the server send a receipt when it has successfully processed the frame.
```rust
// TODO
```
### Handling ERROR frames
To handle errors, you can register an error handler
```rust
// TODO
```

### Cargo.toml
```toml
[package]

name = "stomp_test"
version = "0.0.1"
authors = ["your_name_here"]

[[bin]]

name = "stomp_test"

[dependencies.stomp]

stomp = "*"
```

keywords: `Stomp`, `Rust`, `rust-lang`, `rustlang`, `cargo`, `ActiveMQ`, `RabbitMQ`, `HornetQ`, `OpenMQ`, `Message Queue`, `MQ`


## TODO
- [x] Handle message format
- [ ] Sample message flow test with real world use cases
  - Needed for this:
    - test harness
      - run server
      - client to send messages (can be plain tcp client I guess?)
    - Example messages:
      - Connect and subscribe
      - Connect and publish