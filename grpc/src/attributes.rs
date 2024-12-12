#[derive(Debug, Default, Clone)]
struct TODO;

#[derive(Debug, Default, Clone)]
pub struct Attributes {
    items: TODO,
}

impl Attributes {
    pub fn new() -> Self {
        Self { items: TODO }
    }
    pub fn add<T: Send + Sync>(&mut self, val: T) -> Self {
        Self { items: TODO }
    }
    pub fn get<T: Send + Sync>(&self) -> Option<&T> {
        None
    }
}
