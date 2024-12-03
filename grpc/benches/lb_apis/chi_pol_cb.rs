use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use grpc::client::{
    load_balancing::{LbPolicyCallbacks, LbState, Subchannel},
    name_resolution::ResolverUpdate,
    ConnectivityState,
};

use crate::*;

pub struct ChildPolicyCallbacks {
    scs: Arc<Mutex<HashMap<Subchannel, ConnectivityState>>>,
}

impl ChildPolicyCallbacks {
    pub fn new() -> Self {
        Self {
            scs: Arc::default(),
        }
    }
}

impl LbPolicyCallbacks for ChildPolicyCallbacks {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        _: Option<Box<dyn grpc::client::load_balancing::LbConfig>>,
        channel_controller: &mut dyn grpc::client::load_balancing::ChannelControllerCallbacks,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ResolverUpdate::Data(rd) = update else {
            return Err("bad update".into());
        };
        for address in &rd.endpoints[0].addresses {
            let scmap = self.scs.clone();
            let subchannel = channel_controller.new_subchannel(
                &address,
                Box::new(move |subchannel, state, channel_controller| {
                    let mut m = scmap.lock().unwrap();
                    let Some(e) = m.get_mut(&subchannel) else {
                        return;
                    };
                    *e = state.connectivity_state;

                    let picker = Box::new(DummyPicker {});
                    channel_controller.update_picker(LbState {
                        connectivity_state: effective_state(&m),
                        picker,
                    });
                }),
            );
            subchannel.connect();
            self.scs
                .lock()
                .unwrap()
                .insert(subchannel, ConnectivityState::Idle);
        }
        Ok(())
    }
}
