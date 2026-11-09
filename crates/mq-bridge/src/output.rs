//! A session's budgeted output (core backlog C19, plans/m3-desktop.md 2.3).
//!
//! Every message is serialized once, into a writer that refuses more than
//! [`MAX_MESSAGE`] bytes, and its length is reserved against the session's
//! [`BUDGET`] before it is queued as text. The reservation is released when the
//! consumer drops the message. A send that would cross the budget closes the
//! output with `overflow`: the consumer gets one [`Outgoing::Overflow`] and
//! ends the session; every later send fails at once without queueing.

use serde_json::Value;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// The largest single message, serialized.
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;
/// Serialized bytes a session may have queued and not yet consumed.
pub const BUDGET: usize = 8 * 1024 * 1024;

/// Why a send did not queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The output closed (overflow, or the consumer went away).
    Closed,
    /// The message is over [`MAX_MESSAGE`]; nothing was queued or reserved.
    TooLarge,
}

struct Budget {
    used: AtomicUsize,
    closed: AtomicBool,
    limit: usize,
    max_message: usize,
}

/// One message's share of the budget, given back when it is dropped.
pub struct Held {
    budget: Arc<Budget>,
    len: usize,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.len, Ordering::AcqRel);
    }
}

/// What the consumer reads.
pub enum Outgoing {
    /// One serialized message; its reservation lasts until this is dropped.
    Message { text: String, held: Held },
    /// The budget was crossed: end the session (the app opens a new one).
    Overflow,
}

impl Outgoing {
    /// The message's text; `None` for the overflow marker.
    pub fn text(&self) -> Option<&str> {
        match self {
            Outgoing::Message { text, .. } => Some(text),
            Outgoing::Overflow => None,
        }
    }

    /// The message parsed back (tests, logs of shapes); the overflow marker
    /// reads as `{"output": "overflow"}`.
    pub fn json(&self) -> Value {
        match self {
            Outgoing::Message { text, .. } => {
                serde_json::from_str(text).expect("the output only holds JSON")
            }
            Outgoing::Overflow => serde_json::json!({ "output": "overflow" }),
        }
    }
}

/// The sending side, cloned by every task that answers for the session.
#[derive(Clone)]
pub struct Output {
    tx: mpsc::UnboundedSender<Outgoing>,
    budget: Arc<Budget>,
}

impl Output {
    /// A session output with the standard budget, and the consumer's end.
    pub fn channel() -> (Output, mpsc::UnboundedReceiver<Outgoing>) {
        Self::with_budget(BUDGET)
    }

    /// A smaller budget, for tests.
    pub fn with_budget(limit: usize) -> (Output, mpsc::UnboundedReceiver<Outgoing>) {
        Self::with_limits(limit, MAX_MESSAGE)
    }

    /// A smaller budget and message size, for tests.
    pub fn with_limits(
        limit: usize,
        max_message: usize,
    ) -> (Output, mpsc::UnboundedReceiver<Outgoing>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let budget = Arc::new(Budget {
            used: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            limit,
            max_message,
        });
        (Output { tx, budget }, rx)
    }

    /// Serializes `value` once, reserves its length and queues it.
    pub fn send(&self, value: &Value) -> Result<(), SendError> {
        if self.budget.closed.load(Ordering::Acquire) {
            return Err(SendError::Closed);
        }
        let text = serialize(value, self.budget.max_message)?;
        let len = text.len();
        let reserved = self
            .budget
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(len).filter(|&n| n <= self.budget.limit)
            });
        if reserved.is_err() {
            self.overflow();
            return Err(SendError::Closed);
        }
        let held = Held {
            budget: self.budget.clone(),
            len,
        };
        self.tx
            .send(Outgoing::Message { text, held })
            .map_err(|_| SendError::Closed)
    }

    /// Closes the output with `overflow`: one marker for the consumer, then
    /// every send fails.
    pub fn overflow(&self) {
        if !self.budget.closed.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Outgoing::Overflow);
            // The app fell behind the core: an error report (plans/error-reports.md 2).
            mq_runtime::reports::push(crate::core_report("failed", "Overflow"));
        }
    }

    /// Whether the output is closed.
    pub fn is_closed(&self) -> bool {
        self.budget.closed.load(Ordering::Acquire) || self.tx.is_closed()
    }

    /// Bytes reserved now (tests and diagnostics).
    pub fn reserved(&self) -> usize {
        self.budget.used.load(Ordering::Acquire)
    }
}

/// A writer that fails past its cap, so an oversized message is never held as
/// text.
struct Capped(Vec<u8>, usize);

impl io::Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.0.len() + buf.len() > self.1 {
            return Err(io::Error::other("message too large"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialize(value: &Value, max: usize) -> Result<String, SendError> {
    let mut out = Capped(Vec::new(), max);
    serde_json::to_writer(&mut out, value).map_err(|_| SendError::TooLarge)?;
    // serde_json writes UTF-8 only.
    Ok(String::from_utf8(out.0).expect("serde_json writes UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_message_holds_its_bytes_until_dropped() {
        let (out, mut rx) = Output::with_budget(100);
        out.send(&json!({ "id": 1 })).unwrap();
        let len = r#"{"id":1}"#.len();
        assert_eq!(out.reserved(), len);
        let message = rx.try_recv().unwrap();
        assert_eq!(message.text(), Some(r#"{"id":1}"#));
        assert_eq!(out.reserved(), len, "held while the consumer has it");
        drop(message);
        assert_eq!(out.reserved(), 0);
    }

    #[test]
    fn crossing_the_budget_closes_once_and_later_sends_fail() {
        let (out, mut rx) = Output::with_budget(20);
        out.send(&json!({ "a": "0123456789" })).unwrap();
        assert_eq!(
            out.send(&json!({ "b": "0123456789" })),
            Err(SendError::Closed)
        );
        assert_eq!(out.send(&json!(1)), Err(SendError::Closed));
        assert!(out.is_closed());
        assert!(matches!(rx.try_recv(), Ok(Outgoing::Message { .. })));
        assert!(matches!(rx.try_recv(), Ok(Outgoing::Overflow)));
        assert!(rx.try_recv().is_err(), "nothing queued after the marker");
    }

    #[test]
    fn an_oversized_message_is_refused_without_reserving() {
        let (out, mut rx) = Output::channel();
        let big = "x".repeat(MAX_MESSAGE);
        assert_eq!(out.send(&json!({ "big": big })), Err(SendError::TooLarge));
        assert_eq!(out.reserved(), 0);
        assert!(!out.is_closed());
        assert!(rx.try_recv().is_err());
    }
}
