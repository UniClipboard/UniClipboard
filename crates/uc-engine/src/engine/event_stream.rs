use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::{EngineEvent, RefreshReason};

pub struct EventStream {
    receiver: broadcast::Receiver<EngineEvent>,
}

impl EventStream {
    pub async fn next(&mut self) -> Option<EngineEvent> {
        match self.receiver.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(EngineEvent::RefreshRequired {
                reason: RefreshReason::ConsumerLagged,
            }),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct EventSender {
    sender: Arc<Mutex<Option<broadcast::Sender<EngineEvent>>>>,
}

impl EventSender {
    pub(crate) fn send(&self, event: EngineEvent) {
        let sender = match self.sender.lock() {
            Ok(sender) => sender,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(sender) = sender.as_ref() {
            let _ = sender.send(event);
        }
    }

    pub(crate) fn close(&self) {
        let mut sender = match self.sender.lock() {
            Ok(sender) => sender,
            Err(poisoned) => poisoned.into_inner(),
        };
        sender.take();
    }
}

pub(crate) fn event_channel(capacity: usize) -> (EventSender, EventStream) {
    let (sender, receiver) = broadcast::channel(capacity);
    (
        EventSender {
            sender: Arc::new(Mutex::new(Some(sender))),
        },
        EventStream { receiver },
    )
}

#[cfg(test)]
mod tests {
    use crate::{EngineEvent, EngineState, RefreshReason};

    use super::event_channel;

    #[tokio::test]
    async fn lagged_consumer_receives_refresh_before_continuing() {
        let (events, mut stream) = event_channel(2);

        events.send(EngineEvent::StateChanged {
            state: EngineState::Quiescing,
        });
        events.send(EngineEvent::StateChanged {
            state: EngineState::Quiesced,
        });
        events.send(EngineEvent::StateChanged {
            state: EngineState::Suspended,
        });

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::RefreshRequired {
                reason: RefreshReason::ConsumerLagged,
            })
        );
        assert_eq!(
            stream.next().await,
            Some(EngineEvent::StateChanged {
                state: EngineState::Quiesced,
            })
        );
    }
}
