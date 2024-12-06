use std::{collections::HashSet, error::Error, ops::DerefMut};

use grpc::client::{
    load_balancing::{
        ChannelController, LbConfig, LbPolicy, LbPolicyBuilder, LbPolicyOptions, LbState,
        SubchannelUpdate, WorkScheduler,
    },
    name_resolution::{Address, ResolverData, ResolverUpdate},
};

use crate::*;

#[derive(Default)]
pub struct DelegatingPolicy {
    child_manager: ChildManager,
    children: Vec<WrappedChild>,
    //work_scheduler: Arc<dyn WorkScheduler>,
}

pub struct NopWorkScheduler {}
impl WorkScheduler for NopWorkScheduler {
    fn schedule_work(&self) { /* do nothing */
    }
}

impl LbPolicy for DelegatingPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        _: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let ResolverUpdate::Data(rd) = update else {
            return Err("bad update".into());
        };
        self.children = vec![];
        for endpoint in rd.endpoints {
            let mut child = self
                .child_manager
                .new_child(ChildPolicyBuilder {}, Arc::new(NopWorkScheduler {}));
            let mut rd = ResolverData::default();
            rd.endpoints.push(endpoint);
            let _ = child.resolver_update(ResolverUpdate::Data(rd), None, channel_controller);
            self.children.push(child);
        }
        Ok(())
    }

    fn subchannel_update(
        &mut self,
        subchannel: &Subchannel,
        state: &SubchannelState,
        channel_controller: &mut dyn ChannelController,
    ) {
        self.child_manager
            .subchannel_update(subchannel, state, channel_controller);
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        self.child_manager.work(channel_controller);
    }
}

#[derive(Default)]
pub struct ChildManager {
    subchannel_child_map: HashMap<Subchannel, WrappedChild>,
    // todo: track work requests
    children: HashSet<WrappedChild>,
}

// ChildManager implements LbPolicy forwarding
impl ChildManager {
    fn new_child(
        &mut self,
        child_builder: impl LbPolicyBuilder,
        work_scheduler: Arc<dyn WorkScheduler>,
    ) -> WrappedChild {
        let child = child_builder.build(LbPolicyOptions { work_scheduler });
        WrappedChild { child }
    }
    fn subchannel_update(
        &mut self,
        subchannel: &Subchannel,
        state: &SubchannelState,
        channel_controller: &mut dyn ChannelController,
    ) {
        let c = self.subchannel_child_map.get_mut(subchannel).unwrap();
        c.subchannel_update(subchannel, state, channel_controller);
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        // find proper WrappedChild(ren) that requested work.
        let (_, c) = self.subchannel_child_map.iter_mut().next().unwrap();
        c.work(channel_controller);
    }
}

pub struct WrappedChild {
    child: Box<dyn LbPolicy>,
}

impl LbPolicy for WrappedChild {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut wc = WrappedController { channel_controller };
        self.child.resolver_update(update, config, &mut wc)
    }

    fn subchannel_update(
        &mut self,
        update: &Subchannel,
        state: &SubchannelState,
        channel_controller: &mut dyn ChannelController,
    ) {
        let mut wc = WrappedController { channel_controller };
        self.child.subchannel_update(update, state, &mut wc);
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        let mut wc = WrappedController { channel_controller };
        self.child.work(&mut wc);
    }
}

pub struct WrappedController<'a> {
    channel_controller: &'a mut dyn ChannelController,
}

impl<'a> ChannelController for WrappedController<'a> {
    fn new_subchannel(&mut self, address: &Address) -> Subchannel {
        self.channel_controller.new_subchannel(address)
        // TODO: add entry into subchannel->child map.
    }

    fn update_picker(&mut self, update: LbState) {
        self.channel_controller.update_picker(update);
    }

    fn request_resolution(&mut self) {
        self.channel_controller.request_resolution();
    }
}
