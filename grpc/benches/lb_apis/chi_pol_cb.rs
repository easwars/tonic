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
                Box::new(move |sc, scs, cc| {
                    let mut m = scmap.lock().unwrap();
                    m.insert(sc, scs.connectivity_state);
                    let picker = Box::new(DummyPicker {});

                    cc.update_picker(LbState {
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

fn effective_state(m: &HashMap<Subchannel, ConnectivityState>) -> ConnectivityState {
    let mut connectivity_state = ConnectivityState::TransientFailure;

    for (_, con_state) in m.iter() {
        if *con_state == ConnectivityState::Ready {
            connectivity_state = ConnectivityState::Ready;
        } else if *con_state == ConnectivityState::Connecting
            && connectivity_state != ConnectivityState::Ready
        {
            connectivity_state = ConnectivityState::Connecting;
        } else if *con_state == ConnectivityState::Idle
            && connectivity_state != ConnectivityState::Connecting
            && connectivity_state != ConnectivityState::Ready
        {
            connectivity_state = ConnectivityState::Idle;
        } else if connectivity_state != ConnectivityState::Ready
            && connectivity_state != ConnectivityState::Connecting
            && connectivity_state != ConnectivityState::Idle
        {
            connectivity_state = ConnectivityState::TransientFailure;
        }
    }
    connectivity_state
}
