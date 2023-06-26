use parking_lot::RwLock;
use std::collections::HashSet;

#[derive(Default)]
pub struct State {
    /// A list of passwords that have previously been accepted, and will forever be accepted
    /// to further attract the bear.
    pub previously_accepted_passwords: StoredPasswords,
}

#[derive(Default)]
pub struct StoredPasswords(RwLock<HashSet<Box<str>>>);

impl StoredPasswords {
    pub fn seen(&self, password: &str) -> bool {
        self.0.read().contains(password)
    }

    pub fn store(&self, password: &str) -> bool {
        self.0.write().insert(Box::from(password.to_string()))
    }
}
