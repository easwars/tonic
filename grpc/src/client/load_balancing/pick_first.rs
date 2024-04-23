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

#[derive(Clone)]
struct Policy {
    ch: Arc<dyn lb::Channel>,
    sc: Option<Arc<dyn Subchannel>>,
}

impl Policy {
    fn pick(&self, _r: Request) -> Result<Pick, Box<dyn Error>> {
        Ok(Pick::new(self.sc.clone().unwrap()))
    }
    fn update(&self, s: ConnectivityState) {
        let sc = self.sc.clone();
        if s == ConnectivityState::Ready {
            let slf = self.clone();
            self.ch
                .update_state(Ok(Arc::new(State::new(s, Arc::new(move |v| slf.pick(v))))));
        }
    }
}

impl lb::Policy for Policy {
    fn resolver_update(&mut self, update: lb::ResolverUpdate) {
        if let Ok(u) = update.update {
            if let Some(e) = u.endpoints.first() {
                if let Some(a) = e.addresses.first() {
                    let slf = self.clone();
                    let sc = self.ch.new_subchannel(a.clone());
                    let old_sc = mem::replace(&mut self.sc, Some(sc.clone()));
                    if let Some(o) = old_sc {
                        o.shutdown();
                    };
                    sc.listen(Box::new(move |s| slf.update(s)));
                    sc.connect();
                }
            }
        }
    }
}
