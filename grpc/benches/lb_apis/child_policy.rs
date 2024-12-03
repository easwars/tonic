use std::collections::HashMap;

use grpc::client::{
    load_balancing::{
        ChannelController, LbConfig, LbPolicy, LbState, Picker, Subchannel, SubchannelState,
    },
    name_resolution::ResolverUpdate,
    ConnectivityState,
};

pub struct ChildPolicy {
    scs: HashMap<Subchannel, ConnectivityState>,
}

impl ChildPolicy {
    pub fn new() -> Self {
        Self {
            scs: HashMap::default(),
        }
    }
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
        if !self.scs.contains_key(subchannel) {
            return;
        }
        let mut connectivity_state = ConnectivityState::TransientFailure;

        for (subchan, con_state) in self.scs.iter_mut() {
            if *subchan == *subchannel {
                *con_state = state.connectivity_state;
            };
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
        let picker = Box::new(DummyPicker {});
        channel_controller.update_picker(LbState {
            connectivity_state,
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
