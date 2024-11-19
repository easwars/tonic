use std::{
    error::Error,
    mem,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::time::sleep;
use tonic::transport::channel;

use crate::{
    client::{
        load_balancing::LbState,
        name_resolution::{Address, ResolverUpdate},
        ConnectivityState,
    },
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
            subchannels: vec![],
            next_addresses: Arc::default(),
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
    next_addresses: Arc<Mutex<Vec<Address>>>,
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

        *self.next_addresses.lock().unwrap() = addresses;
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
        update: &super::SubchannelUpdate,
        channel_controller: &mut dyn ChannelController,
    ) {
        dbg!();

        for sc in &self.subchannels {
            // Ignore updates for subchannels other than our subchannel, or if
            // the state is not Ready.
            if update
                .get(sc)
                .is_some_and(|ss| ss.connectivity_state == ConnectivityState::Ready)
            {
                channel_controller.update_picker(LbState {
                    connectivity_state: ConnectivityState::Ready,
                    picker: Box::new(OneSubchannelPicker { sc: sc.clone() }),
                });
                break;
            }
        }
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        let mut work_items = self.next_addresses.lock().unwrap();

        work_items
            .iter()
            .for_each(|v| println!("Called with {}", v));

        if let Some(address) = work_items.pop() {
            self.subchannels
                .push(channel_controller.new_subchannel(&address));
        }
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
