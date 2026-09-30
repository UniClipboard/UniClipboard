use std::time::Duration;

use quick_panel_core::ports::Live;

use super::DaemonHistory;

impl DaemonHistory {
    pub(super) async fn watch_changes(&self, send: tokio::sync::mpsc::Sender<Live>) {
        use std::sync::Arc;
        use uc_daemon_client::{
            realtime::{RealtimeEvent, RealtimeTopic},
            DaemonWsBridge, DaemonWsBridgeConfig,
        };
        let Ok(context) = self.context() else {
            return;
        };
        let state = context.connection_state();
        let bridge = Arc::new(DaemonWsBridge::new(
            state.clone(),
            DaemonWsBridgeConfig::default(),
        ));
        let mut events = match bridge
            .subscribe(
                "gpui-quick-panel",
                &[
                    RealtimeTopic::Clipboard,
                    RealtimeTopic::FileTransfer,
                    RealtimeTopic::Peers,
                    RealtimeTopic::Setup,
                    RealtimeTopic::ContentLock,
                ],
            )
            .await
        {
            Ok(events) => events,
            Err(_) => {
                tracing::warn!("Quick panel realtime subscription failed");
                return;
            }
        };
        let stop = tokio_util::sync::CancellationToken::new();
        let run = bridge.run(stop.clone());
        tokio::pin!(run);
        let mut discovery = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _=send.closed()=>break,
                _=&mut run=>break,
                _=discovery.tick()=>{if let Ok(connection)=self.resolve(){state.set(connection);}},
                event=events.recv()=>{
                    let Some(event)=event else{break;};
                    match event{
                        // A lock must never be dropped because the queue is full, so it waits for room.
                        RealtimeEvent::ContentLockChanged(change)=>{
                            let live=if change.unlocked{Live::ContentUnlocked}else{Live::ContentLocked};
                            if send.send(live).await.is_err(){break;}
                        }
                        _=>{if send.try_send(Live::Changed).is_err()&&send.is_closed(){break;}}
                    }
                },
            }
        }
        stop.cancel();
    }
}
