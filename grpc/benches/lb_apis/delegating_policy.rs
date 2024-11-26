use std::error::Error;

use grpc::client::{
    load_balancing::{ChannelController, LbConfig, LbPolicy, LbState, SubchannelUpdate},
    name_resolution::{Address, ResolverData, ResolverUpdate},
};

use crate::*;

pub struct DelegatingPolicy {
    children: Vec<Box<dyn LbPolicy>>,
}

impl DelegatingPolicy {
    pub fn new() -> Self {
        Self { children: vec![] }
    }
}

impl LbPolicy for DelegatingPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        _: Option<Box<dyn LbConfig>>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let ResolverUpdate::Data(rd) = update else {
            return Err("bad update".into());
        };
        self.children = vec![];
        let mut wc = WrappedController { channel_controller };
        for endpoint in rd.endpoints {
            let mut child = ChildPolicy::new();
            let mut rd = ResolverData::default();
            rd.endpoints.push(endpoint);
            let _ = child.resolver_update(ResolverUpdate::Data(rd), None, &mut wc);
            self.children.push(Box::new(child));
        }
        Ok(())
    }

    fn subchannel_update(
        &mut self,
        update: &SubchannelUpdate,
        channel_controller: &mut dyn ChannelController,
    ) {
        let mut wc = WrappedController { channel_controller };
        self.children.iter_mut().for_each(|child| {
            child.as_mut().subchannel_update(update, &mut wc);
        });
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        let mut wc = WrappedController { channel_controller };
        self.children.iter_mut().for_each(|child| {
            child.as_mut().work(&mut wc);
        });
    }
}

// This is how the channel's controller would be wrapped by a middle LB policy
// that wants to intercept calls.  This benchmark does not have any specific
// behavior injected.
pub struct WrappedController<'a> {
    channel_controller: &'a mut dyn ChannelController,
}

impl<'a> ChannelController for WrappedController<'a> {
    fn new_subchannel(&mut self, address: &Address) -> Subchannel {
        self.channel_controller.new_subchannel(address)
    }

    fn update_picker(&mut self, update: LbState) {
        self.channel_controller.update_picker(update);
    }

    fn request_resolution(&mut self) {
        self.channel_controller.request_resolution();
    }
}
