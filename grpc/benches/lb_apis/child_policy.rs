use std::{borrow::BorrowMut, collections::HashMap};

use grpc::client::{
    load_balancing::{
        ChannelController, LbConfig, LbPolicy, LbPolicyBuilder, LbState, Picker, Subchannel,
        SubchannelState,
    },
    name_resolution::ResolverUpdate,
    ConnectivityState,
};

use crate::*;

pub struct ChildPolicyBuilder {}

impl LbPolicyBuilder for ChildPolicyBuilder {
    fn build(&self, options: grpc::client::load_balancing::LbPolicyOptions) -> Box<dyn LbPolicy> {
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

impl LbPolicy for ChildPolicy {
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
        //update: &grpc::client::load_balancing::SubchannelUpdate,
        channel_controller: &mut dyn ChannelController,
    ) {
        let Some(e) = self.scs.get_mut(subchannel) else {
            return;
        };
        *e = state.connectivity_state;

        let picker = Box::new(DummyPicker {});
        channel_controller.update_picker(LbState {
            connectivity_state: effective_state(&self.scs),
            picker,
        });
    }

    fn work(&mut self, _: &mut dyn ChannelController) {
        todo!()
    }
}

pub struct DummyPicker {}
impl Picker for DummyPicker {
    fn pick(
        &self,
        _: &grpc::service::Request,
    ) -> Result<grpc::client::load_balancing::Pick, Box<dyn std::error::Error>> {
        Err("not valid".into())
    }
}
