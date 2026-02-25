use std::io::{self, ErrorKind};

use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::{Message, error::{Error as WsError, ProtocolError}}};
use futures_util::{SinkExt, StreamExt};


// change if required
const MAX_BROADCAST_CAPACITY: usize = 32;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    println!("Enter port (1024–65535): ");

    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read input");

    let port: u16 = input.trim().parse().expect("Invalid port");

    let listener = match TcpListener::bind(("0.0.0.0", port)).await { // See https://doc.rust-lang.org/std/io/enum.ErrorKind.html
        Ok(l) => l,
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            eprintln!("Port {} in use.", port);
            std::process::exit(1);
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            eprintln!("Permission denied on port {}.", port);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Bind failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("Listening on ws://localhost:{}", port);

    let (tx, _) = broadcast::channel::<Message>(MAX_BROADCAST_CAPACITY);

    loop {
        let (tcp_stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Accept failed: {}", e);
                continue;
            }
        };

        println!("New connection from {}", peer);

        let tx = tx.clone();

        tokio::spawn(async move {
            let ws_stream = match accept_async(tcp_stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("WebSocket connection failed with {}: {}", peer, e);
                    return;
                }
            };

            let (mut ws_sink, mut ws_source) = ws_stream.split();
            let mut rx = tx.subscribe();

            let tx_incoming = tx.clone();
            let forward_to_others = async move {
                while let Some(msg) = ws_source.next().await {
                    let msg = match msg {
                        Ok(m) => m,
                        Err(WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => break,
                        Err(e) => {
                            eprintln!("Read error from {}: {}", peer, e);
                            break;
                        }
                    };

                    match msg { // echo messages, used for communication with peers.
                        Message::Text(text) => {
                            let _ = tx_incoming.send(Message::Text(text.into()));
                        }
                        Message::Binary(data) => {
                            let _ = tx_incoming.send(Message::Binary(data));
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
            };

            let receive_broadcast = async move {
                while let Ok(msg) = rx.recv().await {
                    if ws_sink.send(msg).await.is_err() {
                        break;
                    }
                }
            };

            tokio::select! {
                _ = forward_to_others => {},
                _ = receive_broadcast => {},
            }

            println!("Connection closed: {}", peer);
        });
    }
}