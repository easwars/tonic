#[derive(Debug, Default)]
struct TODO;

#[derive(Debug, Default)]
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
