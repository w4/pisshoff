use std::{borrow::Cow, collections::HashSet};

use parking_lot::RwLock;

#[derive(Default)]
pub struct State {
    /// A list of passwords that have previously been accepted, and will forever be accepted
    /// to further attract the bear.
    pub previously_accepted_passwords: StoredPasswords,
}

#[derive(Default)]
pub struct StoredPasswords(RwLock<HashSet<UsernamePasswordTuple<'static>>>);

impl StoredPasswords {
    pub fn seen(&self, username: &str, password: &str) -> bool {
        self.0
            .read()
            .contains(&UsernamePasswordTuple::new(username, password))
    }

    pub fn store(&self, username: &str, password: &str) -> bool {
        self.0
            .write()
            .insert(UsernamePasswordTuple::new(username, password).into_owned())
    }
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
struct UsernamePasswordTuple<'a> {
    pub username: Cow<'a, str>,
    pub password: Cow<'a, str>,
}

impl<'a> UsernamePasswordTuple<'a> {
    pub fn new(username: impl Into<Cow<'a, str>>, password: impl Into<Cow<'a, str>>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn into_owned(self) -> UsernamePasswordTuple<'static> {
        UsernamePasswordTuple {
            username: Cow::Owned(self.username.into_owned()),
            password: Cow::Owned(self.password.into_owned()),
        }
    }
}
