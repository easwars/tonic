use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chi_pol_single::DummyPicker;
use grpc::client::{
    load_balancing::{
        LbConfig, LbPolicyBuilderCallbacks as LbPolicyBuilder, LbPolicyCallbacks as LbPolicy,
        LbPolicyOptions, LbState, Subchannel,
    },
    name_resolution::ResolverUpdate,
    ConnectivityState,
};

use crate::*;

#[derive(Default)]
pub struct ChildPolicy {
    scs: Arc<Mutex<HashMap<Subchannel, ConnectivityState>>>,
}

pub struct ChildPolicyBuilder {}

impl LbPolicyBuilder for ChildPolicyBuilder {
    fn build(&self, _options: LbPolicyOptions) -> Box<dyn LbPolicy> {
        Box::new(ChildPolicy::default())
    }

    fn name(&self) -> &'static str {
        "child"
    }
}

impl LbPolicy for ChildPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        _: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelControllerCallbacks,
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
