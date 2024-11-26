use grpc::client::{
    load_balancing::{ChannelControllerCallbacks, LbPolicyCallbacks},
    name_resolution::{ResolverData, ResolverUpdate},
};

use crate::*;

pub struct DelegatingPolicyCallbacks {
    children: Vec<Box<dyn LbPolicyCallbacks>>,
}

impl DelegatingPolicyCallbacks {
    pub fn new() -> Self {
        Self { children: vec![] }
    }
}

impl LbPolicyCallbacks for DelegatingPolicyCallbacks {
    fn resolver_update(
        &mut self,
        update: grpc::client::name_resolution::ResolverUpdate,
        _: Option<Box<dyn grpc::client::load_balancing::LbConfig>>,
        channel_controller: &mut dyn grpc::client::load_balancing::ChannelControllerCallbacks,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ResolverUpdate::Data(rd) = update else {
            return Err("bad update".into());
        };
        self.children = vec![];
        let mut wc = WrappedControllerCallbacks { channel_controller };
        for endpoint in rd.endpoints {
            let mut child = ChildPolicyCallbacks::new();
            let mut rd = ResolverData::default();
            rd.endpoints.push(endpoint);
            let _ = child.resolver_update(ResolverUpdate::Data(rd), None, &mut wc);
            self.children.push(Box::new(child));
        }
        Ok(())
    }
}

// This is how the channel's controller would be wrapped by a middle LB policy
// that wants to intercept calls.  This benchmark does not have any specific
// behavior injected.
pub struct WrappedControllerCallbacks<'a> {
    channel_controller: &'a mut dyn ChannelControllerCallbacks,
}

impl<'a> ChannelControllerCallbacks for WrappedControllerCallbacks<'a> {
    fn new_subchannel(
        &mut self,
        address: &grpc::client::name_resolution::Address,
        updates: Box<
            dyn Fn(
                    grpc::client::load_balancing::Subchannel,
                    grpc::client::load_balancing::SubchannelState,
                    &mut dyn ChannelControllerCallbacks,
                ) + Send
                + Sync,
        >,
    ) -> grpc::client::load_balancing::Subchannel {
        self.channel_controller.new_subchannel(address, updates)
    }

    fn update_picker(&mut self, update: grpc::client::load_balancing::LbState) {
        self.channel_controller.update_picker(update);
    }

    fn request_resolution(&mut self) {
        self.channel_controller.request_resolution();
    }
}
