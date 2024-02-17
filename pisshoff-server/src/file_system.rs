#![allow(dead_code)]

use std::{
    borrow::Cow,
    collections::{btree_map::Entry, BTreeMap},
    fmt::{Display, Formatter},
    path::{Path, PathBuf},
};

/// A fake file system, stored in memory only active for the current session.
pub struct FileSystem {
    pwd: PathBuf,
    home: PathBuf,
    data: Tree,
}

pub enum Tree {
    Directory(BTreeMap<String, Box<Tree>>),
    File(Box<[u8]>),
}

impl FileSystem {
    pub fn new(user: &str) -> Self {
        let pwd = if user == "root" {
            PathBuf::from("/root")
        } else {
            PathBuf::from("/home").join(user)
        };

        let mut this = Self {
            home: pwd.clone(),
            pwd,
            data: Tree::Directory(BTreeMap::new()),
        };

        let _res = this.mkdirall(&this.pwd.clone());
        this
    }

    pub fn mkdirall(&mut self, path: &Path) -> Result<(), LsError> {
        let mut tree = &mut self.data;

        for c in path {
            match tree {
                Tree::Directory(d) => {
                    tree = d
                        .entry(c.to_str().unwrap().to_string())
                        .or_insert_with(|| Box::new(Tree::Directory(BTreeMap::new())));
                }
                Tree::File(_) => return Err(LsError::FileExists),
            }
        }

        Ok(())
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

    pub fn read(&self, path: &Path) -> Result<&[u8], LsError> {
        let canonical = self.pwd().join(path);
        let mut tree = &self.data;

        for c in &canonical {
            match tree {
                Tree::Directory(d) => {
                    tree = d
                        .get(c.to_str().unwrap())
                        .ok_or(LsError::NoSuchFileOrDirectory)?;
                }
                Tree::File(_) => {
                    return Err(LsError::NotDirectory);
                }
            }
        }

        match tree {
            Tree::Directory(_) => Err(LsError::IsADirectory),
            Tree::File(content) => Ok(content),
        }
    }

    pub fn write(&mut self, path: &Path, content: Box<[u8]>) -> Result<(), LsError> {
        let canonical = self.pwd().join(path);
        let mut tree = &mut self.data;

        if let Some(parents) = canonical.parent() {
            for c in parents {
                match tree {
                    Tree::Directory(d) => {
                        tree = d
                            .get_mut(c.to_str().unwrap())
                            .ok_or(LsError::NoSuchFileOrDirectory)?;
                    }
                    Tree::File(_) => {
                        return Err(LsError::NotDirectory);
                    }
                }
            }
        }

        match tree {
            Tree::Directory(v) => {
                match v.entry(
                    canonical
                        .components()
                        .next_back()
                        .unwrap()
                        .as_os_str()
                        .to_str()
                        .unwrap()
                        .to_string(),
                ) {
                    Entry::Vacant(v) => {
                        v.insert(Box::new(Tree::File(content)));
                        Ok(())
                    }
                    Entry::Occupied(mut o) if matches!(o.get().as_ref(), Tree::File(_)) => {
                        o.insert(Box::new(Tree::File(content)));
                        Ok(())
                    }
                    Entry::Occupied(_) => Err(LsError::IsADirectory),
                }
            }
            Tree::File(_) => Err(LsError::NotDirectory),
        }
    }

    #[allow(clippy::unused_self)]
    pub fn ls<'a>(&'a self, dir: Option<&'a Path>) -> Result<Vec<&'a str>, LsError> {
        let canonical = if let Some(dir) = dir {
            Cow::Owned(self.pwd().join(dir))
        } else {
            Cow::Borrowed(self.pwd())
        };

        let mut tree = &self.data;

        for c in canonical.as_ref() {
            match tree {
                Tree::Directory(d) => {
                    tree = d
                        .get(c.to_str().unwrap())
                        .ok_or(LsError::NoSuchFileOrDirectory)?;
                }
                Tree::File(_) => {
                    return Err(LsError::NotDirectory);
                }
            }
        }

        match tree {
            Tree::Directory(v) => Ok(v.keys().map(String::as_str).collect()),
            Tree::File(_) => Ok(vec![dir.unwrap_or(self.pwd()).to_str().unwrap()]),
        }
    }
}

#[derive(Debug)]
pub enum LsError {
    NotDirectory,
    NoSuchFileOrDirectory,
    IsADirectory,
    FileExists,
}

impl Display for LsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LsError::NoSuchFileOrDirectory => "No such file or directory",
            LsError::NotDirectory => "Not a directory",
            LsError::IsADirectory => "Is a directory",
            LsError::FileExists => "File exists",
        })
    }
}
