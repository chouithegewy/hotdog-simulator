// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! A socket's way out: the game loop hands each socket its messages here and never waits for the network. Each
//! socket's own task (transport.rs) writes them in order. A socket that falls more than MAX_QUEUED bytes behind
//! (a client that stopped reading) is cut off, as the Node server cut off one whose write buffer passed 1 MiB.
//!
//! In tests a `Memory` outbox keeps what it is sent instead.
use axum::body::Bytes;
use axum::extract::ws::Utf8Bytes;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, mpsc};

/// The most a socket may have waiting to be written.
pub const MAX_QUEUED: usize = 1 << 20;

/// What a socket's task is asked to do, in order.
pub enum Out {
    Text(Utf8Bytes),
    /// A game socket's frame (kitchen_sim::net).
    Binary(Bytes),
    /// Close the socket once what was sent before has gone (JavaScript's `socket.end()`).
    End,
}
pub struct SocketOut {
    pub tx: mpsc::UnboundedSender<Out>,
    pub queued: AtomicUsize,
    /// Cut off at once, whatever is waiting (`socket.destroy()`).
    pub kill: Notify,
    pub killed: AtomicBool,
}
/// What a `Memory` outbox has been sent: the text messages, and the binary ones.
pub type Inbox = Arc<Mutex<Vec<String>>>;
pub type Binaries = Arc<Mutex<Vec<Vec<u8>>>>;
#[derive(Clone)]
pub enum Outbox {
    Socket(Arc<SocketOut>),
    Memory(Inbox, Binaries),
}
impl Outbox {
    /// A socket's outbox, and the receiving end its task writes from.
    pub fn socket() -> (Outbox, Arc<SocketOut>, mpsc::UnboundedReceiver<Out>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let out = Arc::new(SocketOut { tx, queued: AtomicUsize::new(0), kill: Notify::new(), killed: AtomicBool::new(false) });
        (Outbox::Socket(out.clone()), out, rx)
    }
    /// An outbox that keeps what it is sent (tests): its text messages.
    pub fn memory() -> (Outbox, Inbox) {
        let (outbox, inbox, _) = Outbox::memories();
        (outbox, inbox)
    }
    /// An outbox that keeps what it is sent (tests): its text messages and its binary ones.
    pub fn memories() -> (Outbox, Inbox, Binaries) {
        let (inbox, binaries) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(vec![])));
        (Outbox::Memory(inbox.clone(), binaries.clone()), inbox, binaries)
    }
    /// Queues a message of `size` bytes, unless the socket is gone or too far behind (then it is cut off).
    fn queue(&self, out: &SocketOut, size: usize, message: Out) {
        if out.killed.load(Ordering::Relaxed) {
            return;
        }
        if out.queued.load(Ordering::Relaxed) + size > MAX_QUEUED {
            self.destroy();
            return;
        }
        out.queued.fetch_add(size, Ordering::Relaxed);
        let _ = out.tx.send(message);
    }
    /// Sends one text message (already encoded; a broadcast shares one copy among every socket).
    pub fn send(&self, text: &Utf8Bytes) {
        match self {
            Outbox::Socket(out) => self.queue(out, text.len(), Out::Text(text.clone())),
            Outbox::Memory(inbox, _) => inbox.lock().unwrap().push(text.as_str().to_string()),
        }
    }
    /// Sends one binary message.
    pub fn send_binary(&self, bytes: Vec<u8>) {
        match self {
            Outbox::Socket(out) => self.queue(out, bytes.len(), Out::Binary(Bytes::from(bytes))),
            Outbox::Memory(_, binaries) => binaries.lock().unwrap().push(bytes),
        }
    }
    pub fn send_text(&self, text: String) {
        self.send(&Utf8Bytes::from(text));
    }
    /// Closes the socket after what has been sent.
    pub fn end(&self) {
        if let Outbox::Socket(out) = self {
            let _ = out.tx.send(Out::End);
        }
    }
    /// Cuts the socket off now.
    pub fn destroy(&self) {
        if let Outbox::Socket(out) = self {
            out.killed.store(true, Ordering::Relaxed);
            out.kill.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_socket_that_falls_more_than_a_mebibyte_behind_is_cut_off_and_sent_nothing_more() {
        let (outbox, out, mut rx) = Outbox::socket();
        let text = Utf8Bytes::from("x".repeat(100_000));
        for _ in 0..10 {
            outbox.send(&text);
        }
        assert!(!out.killed.load(Ordering::Relaxed));
        assert_eq!(out.queued.load(Ordering::Relaxed), 1_000_000);
        // Its task writes one: there is room again.
        assert!(matches!(rx.try_recv(), Ok(Out::Text(_))));
        out.queued.fetch_sub(text.len(), Ordering::Relaxed);
        outbox.send(&text);
        assert!(!out.killed.load(Ordering::Relaxed));
        // One more than a mebibyte waiting: cut off, and nothing else is queued.
        outbox.send(&text);
        assert!(out.killed.load(Ordering::Relaxed));
        let queued = out.queued.load(Ordering::Relaxed);
        outbox.send(&text);
        assert_eq!(out.queued.load(Ordering::Relaxed), queued);
    }
}
