use clap::Parser;
use serde::{de::DeserializeOwned, Deserialize};
use std::{io::ErrorKind, path::PathBuf, sync::Arc};

/// Parser for command line arguments
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, env, value_parser = load_config::<Config>)]
    pub config: Arc<Config>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

impl Args {
    pub fn verbosity(&self) -> &'static str {
        match self.verbose {
            0 => "info",
            1 => "debug,thrussh=info",
            2 => "debug",
            _ => "trace",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub socket_path: PathBuf,
    pub pg: deadpool_postgres::Config,
}

fn load_config<T: DeserializeOwned>(path: &str) -> Result<Arc<T>, std::io::Error> {
    let file = std::fs::read_to_string(path)?;

    toml::from_str(&file)
        .map(Arc::new)
        .map_err(|e| std::io::Error::new(ErrorKind::Other, e))
}
