use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use grpc::client::{
    load_balancing::{
        ChannelControllerCallbacks as ChannelController, LbConfig,
        LbPolicyBuilderCallbacks as LbPolicyBuilder, LbPolicyCallbacks as LbPolicy,
        LbPolicyOptions, LbState, Subchannel,
    },
    name_resolution::ResolverUpdate,
    ConnectivityState,
};

use crate::{chi_pol_single::DummyPicker, effective_state};

pub struct ChildPolicy {
    channel_controller: Arc<dyn ChannelController>,
    scs: Arc<Mutex<HashMap<Subchannel, ConnectivityState>>>,
}

pub struct ChildPolicyBuilder {}

impl LbPolicyBuilder for ChildPolicyBuilder {
    fn build(
        &self,
        _options: LbPolicyOptions,
        channel_controller: Arc<dyn ChannelController>,
    ) -> Box<dyn LbPolicy> {
        Box::new(ChildPolicy {
            channel_controller,
            scs: Arc::default(),
        })
    }

    fn name(&self) -> &'static str {
        "child"
    }
}

impl LbPolicy for ChildPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        _: Option<&LbConfig>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ResolverUpdate::Data(rd) = update else {
            return Err("bad update".into());
        };
        for address in &rd.endpoints[0].addresses {
            let scmap = self.scs.clone();
            let channel_controller = self.channel_controller.clone();
            let subchannel = self.channel_controller.new_subchannel(
                &address,
                Box::new(move |subchannel, state| {
                    let mut m = scmap.lock().unwrap();
                    let Some(e) = m.get_mut(&subchannel) else {
                        return;
                    };
                    *e = state.connectivity_state;

                    let picker = Arc::new(DummyPicker {});
                    channel_controller.update_picker(LbState {
                        connectivity_state: effective_state(m.iter().map(|(_, v)| *v)),
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
