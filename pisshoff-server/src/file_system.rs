#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// A fake file system, stored in memory only active for the current session.
pub struct FileSystem {
    pwd: PathBuf,
    home: PathBuf,
}

impl FileSystem {
    pub fn new(user: &str) -> Self {
        let pwd = if user == "root" {
            PathBuf::new().join("/root")
        } else {
            PathBuf::new().join("/home").join(user)
        };

        Self {
            home: pwd.clone(),
            pwd,
        }
    }

    pub fn cd(&mut self, v: Option<&str>) {
        if let Some(v) = v {
            self.pwd.push(v);
        } else {
            self.pwd = self.home.clone();
        }
    }

    pub fn pwd(&self) -> &Path {
        &self.pwd
    }

    #[allow(clippy::unused_self)]
    pub fn ls(&self, _dir: Option<&str>) -> &[&str] {
        &[]
    }
}
