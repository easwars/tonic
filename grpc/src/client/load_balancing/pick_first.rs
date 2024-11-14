use std::error::Error;

use crate::{
    client::{load_balancing::LbState, name_resolution::ResolverUpdate, ConnectivityState},
    service::Request,
};

use super::{
    ChannelController, LbConfig, LbPolicy, LbPolicyBuilder, LbPolicyOptions, Pick, Picker,
    Subchannel, WorkScheduler,
};

pub static POLICY_NAME: &str = "pick_first";

struct Builder {}

impl LbPolicyBuilder for Builder {
    fn build(&self, options: LbPolicyOptions) -> Box<dyn LbPolicy> {
        Box::new(PickFirstPolicy {
            work_scheduler: options.work_scheduler,
            subchannel: None,
        })
    }

    fn name(&self) -> &'static str {
        POLICY_NAME
    }
}

pub fn reg() {
    super::GLOBAL_LB_REGISTRY.add_builder(Builder {})
}

struct PickFirstPolicy {
    work_scheduler: Box<dyn WorkScheduler>,
    subchannel: Option<Subchannel>,
}

impl LbPolicy for PickFirstPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<Box<dyn LbConfig>>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let ResolverUpdate::Data(update) = update else {
            return Err("unhandled".into());
        };
        let address = update
            .endpoints
            .first()
            .ok_or("no endpoints")?
            .addresses
            .first()
            .ok_or("no addresses")?;
        let sc = channel_controller.new_subchannel(address);
        self.subchannel = Some(sc.clone());
        sc.connect();
        self.work_scheduler
            .schedule_work(Box::new("call me maybe?"));
        // TODO: return a picker that queues RPCs.
        Ok(())
    }

    fn subchannel_update(
        &mut self,
        update: &super::SubchannelUpdate,
        channel_controller: &mut dyn ChannelController,
    ) {
        dbg!();

        let Some(sc) = self.subchannel.clone() else {
            return;
        };
        if update
            .get(&sc)
            .is_none_or(|ss| ss.connectivity_state != ConnectivityState::Ready)
        {
            // Ignore updates for subchannels other than our subchannel, or if
            // the state is not Ready.
            return;
        }

        channel_controller.update_picker(LbState {
            connectivity_state: ConnectivityState::Ready,
            picker: Box::new(OneSubchannelPicker { sc }),
        });
    }

    fn work(&mut self, _: &mut dyn ChannelController, data: Box<dyn std::any::Any + Send + Sync>) {
        println!("Called with {}", data.downcast_ref::<&str>().unwrap());
    }
}

struct OneSubchannelPicker {
    sc: Subchannel,
}

impl Picker for OneSubchannelPicker {
    fn pick(&self, request: &Request) -> Result<Pick, Box<dyn Error>> {
        Ok(Pick {
            subchannel: self.sc.clone(),
            on_complete: None,
            metadata: None,
        })
    }
}
