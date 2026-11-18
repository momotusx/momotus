//! `tracing` to the host's `log` (`ares-inspect --service` shows it), and to
//! the error reports' layer, as the native shells wire them. Our crates log
//! from `info`; others, whose debug lines name hosts, from `warn`.

use std::fmt::Write;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

pub(crate) fn init() {
    let subscriber = tracing_subscriber::registry()
        .with(HostLog)
        .with(mq_runtime::reports::layer());
    let _ = tracing::subscriber::set_global_default(subscriber);
}

struct HostLog;

impl<S: Subscriber> Layer<S> for HostLog {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        let level = *meta.level();
        let floor = if meta.target().starts_with("mq_") {
            Level::INFO
        } else {
            Level::WARN
        };
        if level > floor {
            return;
        }
        let mut line = String::from(meta.target());
        event.record(&mut Line(&mut line));
        let number = match level {
            Level::ERROR => 1,
            Level::WARN => 2,
            Level::INFO => 3,
            Level::DEBUG => 4,
            Level::TRACE => 5,
        };
        mq_rt::log(number, &line);
    }
}

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, " {value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}
