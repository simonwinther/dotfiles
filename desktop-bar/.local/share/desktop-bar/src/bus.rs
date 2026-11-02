//! Filter bus traffic at the daemon and bound complete service queries.
use futures_lite::{Stream, future};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use zbus::{
    AsyncDrop, Message, MessageStream,
    blocking::{Connection, MessageIterator},
};

pub fn query<T>(read: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    async_io::block_on(future::race(read, async {
        async_io::Timer::after(Duration::from_secs(3)).await;
        Err(zbus::Error::Failure(
            "Desktop service did not respond".into(),
        ))
    }))
}

pub struct Signals {
    streams: Vec<MessageStream>,
    next: usize,
}
impl Signals {
    pub fn new(connection: &Connection, rules: &[&str]) -> zbus::Result<Self> {
        let mut signals = Self {
            streams: vec![],
            next: 0,
        };
        for &rule in rules {
            signals
                .streams
                .push(MessageIterator::for_match_rule(rule, connection, Some(32))?.into_inner());
        }
        Ok(signals)
    }
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<zbus::Result<Message>>> {
        if self.streams.is_empty() {
            return Poll::Ready(None);
        }
        for offset in 0..self.streams.len() {
            let index = (self.next + offset) % self.streams.len();
            if let Poll::Ready(message) = Pin::new(&mut self.streams[index]).poll_next(cx) {
                self.next = (index + 1) % self.streams.len();
                return Poll::Ready(message);
            }
        }
        Poll::Pending
    }
}
impl Iterator for Signals {
    type Item = zbus::Result<Message>;
    fn next(&mut self) -> Option<Self::Item> {
        async_io::block_on(future::poll_fn(|cx| self.poll_next(cx)))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for stream in self.streams.drain(..) {
            async_io::block_on(stream.async_drop());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, time::Instant};

    #[test]
    fn queries_have_a_total_deadline() {
        assert_eq!(query(async { Ok(7) }).unwrap(), 7);
        let started = Instant::now();
        let error = query(future::pending::<zbus::Result<()>>()).unwrap_err();
        assert!(error.to_string().contains("did not respond"));
        assert!((2.9..5.0).contains(&started.elapsed().as_secs_f32()));
    }

    #[test]
    fn independent_signal_rules_filter_noise_and_keep_owner_changes() {
        if std::env::var_os("DESKTOP_BAR_BUS_TEST").is_none() {
            let directory = tempfile::tempdir().unwrap();
            let config = directory.path().join("session.conf");
            std::fs::write(
                &config,
                r#"<busconfig>
                <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
                <policy context="default"><allow send_destination="*"/>
                <allow receive_sender="*"/><allow own="*"/></policy>
                </busconfig>"#,
            )
            .unwrap();
            let status = std::process::Command::new("dbus-run-session")
                .arg(format!("--config-file={}", config.display()))
                .args([
                    "--",
                    std::env::current_exe().unwrap().to_str().unwrap(),
                    "--exact",
                    "bus::tests::independent_signal_rules_filter_noise_and_keep_owner_changes",
                    "--nocapture",
                ])
                .env("DESKTOP_BAR_BUS_TEST", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let receiver = Connection::session().unwrap();
        let sender = Connection::session().unwrap();
        let mut signals = Signals::new(&receiver, &[
            "type='signal',interface='org.example.Filtered.First'",
            "type='signal',interface='org.example.Filtered.Second'",
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.example.Filtered'",
        ]).unwrap();
        for _ in 0..10 {
            sender
                .emit_signal(None::<&str>, "/test", "org.example.Noise", "Tick", &())
                .unwrap();
        }
        let timed_next = |signals: &mut Signals| {
            async_io::block_on(future::race(
                future::poll_fn(|cx| signals.poll_next(cx)),
                async {
                    async_io::Timer::after(Duration::from_secs(1)).await;
                    None
                },
            ))
        };
        assert!(timed_next(&mut signals).is_none());
        for interface in ["org.example.Filtered.First", "org.example.Filtered.Second"] {
            sender
                .emit_signal(None::<&str>, "/test", interface, "Changed", &())
                .unwrap();
        }
        let mut received = HashSet::new();
        for _ in 0..2 {
            let message = timed_next(&mut signals).unwrap().unwrap();
            received.insert(message.header().interface().unwrap().to_string());
        }
        assert_eq!(
            received,
            HashSet::from([
                "org.example.Filtered.First".into(),
                "org.example.Filtered.Second".into()
            ])
        );
        sender
            .request_name("org.example.Filtered.Application")
            .unwrap();
        let message = timed_next(&mut signals).unwrap().unwrap();
        assert_eq!(
            message.header().member().unwrap().as_str(),
            "NameOwnerChanged"
        );
        assert_eq!(
            message
                .body()
                .deserialize::<(String, String, String)>()
                .unwrap()
                .0,
            "org.example.Filtered.Application"
        );
    }
}
