use std::{borrow::BorrowMut, collections::HashMap};

use grpc::{
    client::{
        load_balancing::{
            ChannelController, LbConfig, LbPolicyBuilderSingle, LbPolicySingle, LbState,
            PickResult, Picker, Subchannel, SubchannelState,
        },
        name_resolution::ResolverUpdate,
        ConnectivityState,
    },
    service::Request,
};

use crate::*;

pub struct ChildPolicyBuilder {}

impl LbPolicyBuilderSingle for ChildPolicyBuilder {
    fn build(
        &self,
        _options: grpc::client::load_balancing::LbPolicyOptions,
    ) -> Box<dyn LbPolicySingle> {
        Box::new(ChildPolicy::default())
    }

    fn name(&self) -> &'static str {
        "child"
    }
}

#[derive(Default)]
struct ChildPolicy {
    scs: HashMap<Subchannel, ConnectivityState>,
}

impl LbPolicySingle for ChildPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        _: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ResolverUpdate::Data(rd) = update else {
            return Err("bad update".into());
        };
        for address in &rd.endpoints[0].addresses {
            let subchannel = channel_controller.new_subchannel(&address);
            subchannel.connect();
            self.scs.insert(subchannel, ConnectivityState::Idle);
        }
        Ok(())
    }

    fn subchannel_update(
        &mut self,
        subchannel: &Subchannel,
        state: &SubchannelState,
        channel_controller: &mut dyn ChannelController,
    ) {
        let Some(e) = self.scs.get_mut(subchannel) else {
            return;
        };
        *e = state.connectivity_state;

        let picker = Arc::new(DummyPicker {});
        channel_controller.update_picker(LbState {
            connectivity_state: effective_state(self.scs.iter().map(|(_, v)| *v)),
            picker,
        });
    }

    fn work(&mut self, _: &mut dyn ChannelController) {
        todo!()
    }
}

pub struct DummyPicker {}
impl Picker for DummyPicker {
    fn pick(&self, _: &Request) -> PickResult {
        PickResult::Err("not valid".into())
    }
}
