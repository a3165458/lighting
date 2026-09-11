//! Interrupt TCP backpressure before waiting for the encoder and restoring displays.

use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct ClientConnection {
    socket: Arc<TcpStream>,
    stop: Arc<AtomicBool>,
    monitor: tokio::task::JoinHandle<()>,
}

impl ClientConnection {
    /// `socket` must be a duplicate of the streaming socket: shutdown affects
    /// both halves even while the video thread is blocked in write_all.
    pub fn new(socket: TcpStream, session_stop: Arc<AtomicBool>) -> Self {
        let socket = Arc::new(socket);
        let stop = Arc::new(AtomicBool::new(session_stop.load(Ordering::Relaxed)));
        let watched_socket = socket.clone();
        let client_stop = stop.clone();
        let monitor = tokio::spawn(async move {
            while !session_stop.load(Ordering::Relaxed) && !client_stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            client_stop.store(true, Ordering::Relaxed);
            let _ = watched_socket.shutdown(Shutdown::Both);
        });
        Self {
            socket,
            stop,
            monitor,
        }
    }

    /// Client EOF/heartbeat failure stops only this client, not the accept loop.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.socket.shutdown(Shutdown::Both);
        self.monitor.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    async fn connected() -> (TcpStream, tokio::net::TcpStream, tokio::net::TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let server = server.into_std().unwrap();
        let shutdown = server.try_clone().unwrap();
        (
            shutdown,
            tokio::net::TcpStream::from_std(server).unwrap(),
            client,
        )
    }

    #[tokio::test]
    async fn suspend_interrupts_a_backpressured_stream() {
        let (shutdown, mut server, _non_reading_tablet) = connected().await;
        let session_stop = Arc::new(AtomicBool::new(false));
        let _connection = ClientConnection::new(shutdown, session_stop.clone());
        let payload = vec![0u8; 16 * 1024 * 1024];
        assert!(
            timeout(Duration::from_millis(100), server.write_all(&payload))
                .await
                .is_err()
        );
        session_stop.store(true, Ordering::Relaxed);
        assert!(timeout(Duration::from_secs(2), server.write_all(&payload))
            .await
            .unwrap()
            .is_err());
    }

    #[tokio::test]
    async fn client_disconnect_does_not_stop_next_client() {
        let (shutdown, mut server, mut tablet) = connected().await;
        let session_stop = Arc::new(AtomicBool::new(false));
        let connection = ClientConnection::new(shutdown, session_stop.clone());
        connection.stop_flag().store(true, Ordering::Relaxed);
        let mut byte = [0];
        assert_eq!(
            timeout(Duration::from_secs(2), tablet.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert!(!session_stop.load(Ordering::Relaxed));
        assert!(server.write_all(b"stale frame").await.is_err());

        let (shutdown, mut next_server, mut next_tablet) = connected().await;
        let _next_connection = ClientConnection::new(shutdown, session_stop);
        next_server.write_all(b"next frame").await.unwrap();
        let mut frame = [0; 10];
        timeout(Duration::from_secs(2), next_tablet.read_exact(&mut frame))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&frame, b"next frame");
    }

    #[tokio::test]
    async fn early_error_closes_socket_without_waiting_for_monitor() {
        let (shutdown, _server, mut tablet) = connected().await;
        let connection = ClientConnection::new(shutdown, Arc::new(AtomicBool::new(false)));
        drop(connection);
        let mut byte = [0];
        assert_eq!(
            timeout(Duration::from_secs(2), tablet.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
}
