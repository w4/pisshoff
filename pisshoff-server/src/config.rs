use std::{io::ErrorKind, net::SocketAddr, path::PathBuf, sync::Arc};

use clap::Parser;
use serde::{de::DeserializeOwned, Deserialize};

/// Parser for command line arguments, these arguments can also be passed via capitalised env vars
/// of the same name.
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

#[derive(Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// Address for the server to listen on.
    #[serde(default = "Config::default_listen_address")]
    pub listen_address: SocketAddr,
    /// The probability that an authentication attempt will succeed, once a given password
    /// has been accepted once - it will be accepted for the rest of the lifetime of the
    /// instance.
    #[serde(default = "Config::default_access_probability")]
    pub access_probability: f64,
    /// Path of the file to write audit logs to.
    #[serde(default = "Config::default_audit_output_file")]
    pub audit_output_file: PathBuf,
    /// The server ID string sent at the beginning of the SSH connection.
    #[serde(default = "Config::default_server_id")]
    pub server_id: String,
}

impl Config {
    fn default_listen_address() -> SocketAddr {
        "0.0.0.0:22".parse().unwrap()
    }

    fn default_access_probability() -> f64 {
        0.2
    }

    fn default_audit_output_file() -> PathBuf {
        "/var/log/pisshoff/audit.log".parse().unwrap()
    }

    fn default_server_id() -> String {
        "SSH-2.0-OpenSSH_9.3".to_string()
    }
}

fn load_config<T: DeserializeOwned>(path: &str) -> Result<Arc<T>, std::io::Error> {
    let file = std::fs::read_to_string(path)?;

    toml::from_str(&file)
        .map(Arc::new)
        .map_err(|e| std::io::Error::new(ErrorKind::Other, e))
}
