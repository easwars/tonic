use std::{error::Error, mem, sync::Arc};

use crate::{
    client::{
        load_balancing::{self as lb, State},
        ConnectivityState,
    },
    service::Request,
};

use super::{Pick, Subchannel};

pub static POLICY_NAME: &str = "pick_first";

struct Builder {}

impl lb::Builder for Builder {
    fn build(&self, channel: Arc<dyn lb::Channel>, options: lb::TODO) -> Box<dyn lb::Policy> {
        Box::new(Policy {
            ch: channel,
            sc: None,
        })
    }

    fn name(&self) -> &'static str {
        POLICY_NAME
    }
}

pub fn reg() {
    super::GLOBAL_REGISTRY.add_builder(&Builder {})
}

#[derive(Clone)]
struct Policy {
    ch: Arc<dyn lb::Channel>,
    sc: Option<Arc<dyn Subchannel>>,
}

impl Policy {
    fn pick(&self, _r: &Request) -> Result<Pick, Box<dyn Error>> {
        Ok(Pick {
            subchannel: self.sc.clone().unwrap(),
            on_complete: None,
            metadata: None,
        })
    }
    fn update(&self, s: ConnectivityState) {
        println!("lb update called: {s:?} -- sc? {}", self.sc.is_some());
        let sc = self.sc.clone();
        if s == ConnectivityState::Ready {
            let slf = self.clone();
            self.ch.update_state(Ok(Box::new(State {
                connectivity_state: s,
                picker: Box::new(move |v| slf.pick(v)),
            })));
        }
    }
}

impl lb::Policy for Policy {
    fn resolver_update(&mut self, update: lb::ResolverUpdate) {
        if let Ok(u) = update.update {
            if let Some(e) = u.endpoints.into_iter().next() {
                if let Some(a) = e.addresses.into_iter().next() {
                    let a = Arc::new(a);
                    let sc = self.ch.new_subchannel(a.clone());
                    println!("prev sc: {}", self.sc.is_some());
                    let old_sc = mem::replace(&mut self.sc, Some(sc.clone()));
                    println!(
                        "old_sc: {}, new sc: {}",
                        old_sc.is_some(),
                        self.sc.is_some()
                    );
                    if let Some(o) = old_sc {
                        o.shutdown();
                    };
                    let slf = self.clone();
                    sc.listen(Box::new(move |s| slf.update(s)));
                    sc.connect();
                }
            }
        }
    }
}
