use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::time::sleep;

use crate::{
    client::{
        load_balancing::LbState,
        name_resolution::{Address, ResolverUpdate},
        subchannel, ConnectivityState,
    },
    service::Request,
};

use super::{
    ChannelController, LbConfig, LbPolicyBuilderSingle, LbPolicyOptions, LbPolicySingle, Pick,
    PickResult, Picker, Subchannel, SubchannelState, WorkScheduler,
};

pub static POLICY_NAME: &str = "pick_first";

struct Builder {}

impl LbPolicyBuilderSingle for Builder {
    fn build(&self, options: LbPolicyOptions) -> Box<dyn LbPolicySingle> {
        Box::new(PickFirstPolicy {
            work_scheduler: options.work_scheduler,
            subchannels: vec![],
            next_addresses: Vec::default(),
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
    work_scheduler: Arc<dyn WorkScheduler>,
    subchannels: Vec<Subchannel>,
    next_addresses: Vec<Address>,
}

impl LbPolicySingle for PickFirstPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<&LbConfig>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let ResolverUpdate::Data(update) = update else {
            return Err("unhandled".into());
        };
        //let endpoints = mem::replace(&mut update.endpoints, vec![]);
        let mut addresses = update
            .endpoints
            .into_iter()
            .next()
            .ok_or("no endpoints")?
            .addresses;

        let address = addresses.pop().ok_or("no addresses")?;

        let sc = channel_controller.new_subchannel(&address);
        self.subchannels = vec![sc.clone()];
        sc.connect();

        self.next_addresses = addresses;
        let work_scheduler = self.work_scheduler.clone();
        // TODO: Implement Drop that cancels this task.
        tokio::task::spawn(async move {
            sleep(Duration::from_millis(200)).await;
            work_scheduler.schedule_work();
        });
        // TODO: return a picker that queues RPCs.
        Ok(())
    }

    fn subchannel_update(
        &mut self,
        subchannel: &Subchannel,
        state: &SubchannelState,
        channel_controller: &mut dyn ChannelController,
    ) {
        dbg!();

        for sc in &self.subchannels {
            // Ignore updates for subchannels other than our subchannel, or if
            // the state is not Ready.
            if *sc == *subchannel && state.connectivity_state == ConnectivityState::Ready {
                channel_controller.update_picker(LbState {
                    connectivity_state: ConnectivityState::Ready,
                    picker: Arc::new(OneSubchannelPicker { sc: sc.clone() }),
                });
                break;
            }
        }
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        println!("Called with {:?}", self.next_addresses);

        if let Some(address) = self.next_addresses.pop() {
            self.subchannels
                .push(channel_controller.new_subchannel(&address));
        }
    }
}

struct OneSubchannelPicker {
    sc: Subchannel,
}

impl Picker for OneSubchannelPicker {
    fn pick(&self, request: &Request) -> PickResult {
        PickResult::Subchannel(Pick {
            subchannel: self.sc.clone(),
            on_complete: None,
            metadata: None,
        })
    }
}
