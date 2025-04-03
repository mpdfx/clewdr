use claude_tokenizer::tokenize;
use colored::Colorize;
use rand::{Rng, rng};
use regex::Regex;
use rquest::Proxy;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display},
    hash::Hash,
};
use tracing::{error, info, warn};

use crate::{Args, error::ClewdrError, utils::config_dir};

pub const CONFIG_NAME: &str = "config.toml";
pub const ENDPOINT: &str = "https://api.claude.ai";
const fn default_max_connections() -> usize {
    16
}

/// A struct representing the configuration of the application
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    // Cookie configurations
    #[serde(default)]
    pub cookie_array: Vec<CookieInfo>,
    pub wasted_cookie: Vec<UselessCookie>,

    // Network settings
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    password: String,
    pub proxy: String,
    ip: String,
    port: u16,

    // Proxy configurations
    pub rproxy: String,

    // Prompt configurations
    pub use_real_roles: bool,
    pub custom_h: Option<String>,
    pub custom_a: Option<String>,
    pub custom_prompt: String,
    pub padtxt_file: String,
    pub padtxt_len: usize,

    // Nested settings section
    #[serde(default)]
    pub settings: Settings,

    // Skip field
    #[serde(skip)]
    pub rquest_proxy: Option<Proxy>,
    #[serde(skip)]
    pub pad_tokens: Vec<String>,
}

/// Reason why a cookie is considered useless
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum Reason {
    Null,
    Disabled,
    Unverified,
    Overlap,
    Banned,
    Invalid,
    Exhausted(i64),
    CoolDown,
}

impl Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reason::Null => write!(f, "Null"),
            Reason::Disabled => write!(f, "Disabled"),
            Reason::Unverified => write!(f, "Unverified"),
            Reason::Overlap => write!(f, "Overlap"),
            Reason::Banned => write!(f, "Banned"),
            Reason::Invalid => write!(f, "Invalid"),
            Reason::Exhausted(i) => write!(f, "Temporarily Exhausted: {}", i),
            Reason::CoolDown => write!(f, "CoolDown"),
        }
    }
}

/// A struct representing a useless cookie
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UselessCookie {
    pub cookie: Cookie,
    pub reason: Reason,
}
impl PartialEq for UselessCookie {
    fn eq(&self, other: &Self) -> bool {
        self.cookie == other.cookie
    }
}
impl Eq for UselessCookie {}
impl Hash for UselessCookie {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.cookie.hash(state);
    }
}

impl UselessCookie {
    pub fn new(cookie: Cookie, reason: Reason) -> Self {
        Self { cookie, reason }
    }
}

/// A struct representing a cookie with its information
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CookieInfo {
    pub cookie: Cookie,
    pub model: Option<String>,
    #[serde(deserialize_with = "validate_reset")]
    #[serde(default)]
    pub reset_time: Option<i64>,
}

impl PartialEq for CookieInfo {
    fn eq(&self, other: &Self) -> bool {
        self.cookie == other.cookie
    }
}
impl Eq for CookieInfo {}
impl Hash for CookieInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.cookie.hash(state);
    }
}

/// Additional settings, ported from clewd, may be merged into config in the future
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Settings {
    pub pass_params: bool,
    pub preserve_chats: bool,
    pub skip_restricted: bool,
}

/// Default cookie value for testing purposes
const PLACEHOLDER_COOKIE: &str = "sk-ant-sid01----------------------------SET_YOUR_COOKIE_HERE----------------------------------------AAAAAAAA";

/// Function to validate the reset time of a cookie while deserializing
fn validate_reset<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // skip no deserializable value
    let Ok(value) = Option::<i64>::deserialize(deserializer) else {
        return Ok(None);
    };
    // skip empty value
    let Some(v) = value else {
        return Ok(None);
    };
    // parse timestamp
    let Some(time) = chrono::DateTime::from_timestamp(v, 0) else {
        warn!("Invalid reset time: {}", v);
        return Ok(None);
    };
    let now = chrono::Utc::now();
    if time < now {
        // cookie have reset
        info!("Cookie reset time is in the past: {}", time);
        return Ok(None);
    }
    let remaining_time = time - now;
    info!("Cookie reset in {} hours", remaining_time.num_hours());
    Ok(Some(v))
}

impl CookieInfo {
    pub fn new(cookie: &str, model: Option<&str>, reset_time: Option<i64>) -> Self {
        Self {
            cookie: Cookie::from(cookie),
            model: model.map(|m| m.to_string()),
            reset_time,
        }
    }
}

/// A struct representing a cookie
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cookie {
    inner: String,
}

impl Default for Cookie {
    fn default() -> Self {
        Self {
            inner: PLACEHOLDER_COOKIE.to_string(),
        }
    }
}

impl Cookie {
    /// Check if the cookie is valid format
    pub fn validate(&self) -> bool {
        // Check if the cookie is valid
        let re = regex::Regex::new(r"sk-ant-sid01-[0-9A-Za-z_-]{86}-[0-9A-Za-z_-]{6}AA").unwrap();
        re.is_match(&self.inner)
    }

    pub fn clear(&mut self) {
        // Clear the cookie
        self.inner.clear();
    }
}

impl From<&str> for Cookie {
    /// Create a new cookie from a string
    fn from(original: &str) -> Self {
        // split off first '@' to keep compatibility with clewd
        let cookie = original.split_once('@').map_or(original, |(_, c)| c);
        // only keep '=' '_' '-' and alphanumeric characters
        let cookie = cookie
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '=' || *c == '_' || *c == '-')
            .collect::<String>()
            .trim_start_matches("sessionKey=")
            .to_string();
        let cookie = Self { inner: cookie };
        if !cookie.validate() {
            warn!("Invalid cookie format: {}", original);
        }
        cookie
    }
}

impl Display for Cookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sessionKey={}", self.inner)
    }
}

impl Debug for Cookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sessionKey={}", self.inner)
    }
}

impl Serialize for Cookie {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let str = self.to_string();
        serializer.serialize_str(&str)
    }
}

impl<'de> Deserialize<'de> for Cookie {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Cookie::from(s.as_str()))
    }
}

/// Generate a random password of given length
fn generate_password(length: usize) -> String {
    println!(
        "{}",
        "Generating random password, paste it to your proxy setting in SillyTavern".green()
    );
    let mut rng = rng();
    (0..length)
        .map(|_| rng.random_range(33..=126) as u8 as char) // 33–126 inclusive
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cookie_array: vec![
                CookieInfo::new(PLACEHOLDER_COOKIE, None, None),
                CookieInfo::new(PLACEHOLDER_COOKIE, Some("claude_pro"), None),
            ],
            wasted_cookie: Vec::new(),
            password: String::new(),
            proxy: String::new(),
            ip: "127.0.0.1".to_string(),
            port: 8484,
            max_connections: default_max_connections(),
            rproxy: String::new(),
            settings: Settings::default(),
            use_real_roles: false,
            custom_prompt: String::new(),
            padtxt_file: String::new(),
            padtxt_len: 4000,
            custom_h: None,
            custom_a: None,
            rquest_proxy: None,
            pad_tokens: Vec::new(),
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // one line per field
        write!(
            f,
            "Password: {}\n\
            Forward Proxy: {}\n\
            Reverse Proxy: {}\n\
            Available Cookies in array: {}\n",
            self.password.yellow(),
            self.proxy.to_string().blue(),
            self.rproxy.to_string().blue(),
            self.cookie_array
                .iter()
                .filter(|x| x.reset_time.is_none())
                .count()
                .to_string()
                .blue()
        )?;
        if !self.pad_tokens.is_empty() {
            Ok(writeln!(
                f,
                "Pad txt token count: {}",
                self.pad_tokens.len().to_string().blue()
            )?)
        } else {
            Ok(())
        }
    }
}

impl Config {
    pub fn auth(&self, key: &str) -> bool {
        if key == self.password { true } else { false }
    }

    /// Load the configuration from the file
    pub fn load() -> Result<Self, ClewdrError> {
        // try to read from pwd
        let file_string = std::fs::read_to_string(CONFIG_NAME).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                // try to read from exec path
                let exec_path = std::env::current_exe()?;
                let config_dir = exec_path.parent().ok_or(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Failed to get parent directory",
                ))?;
                let config_path = config_dir.join(CONFIG_NAME);
                std::fs::read_to_string(config_path)
            } else {
                Err(e)
            }
        });
        match file_string {
            Ok(file_string) => {
                // parse the config file
                let mut config: Config = toml::de::from_str(&file_string)?;
                config.load_from_arg_file();
                config.load_padtxt();
                config = config.validate();
                config.save()?;
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // create a default config file
                let exec_path = std::env::current_exe()?;
                let config_dir = exec_path.parent().ok_or(ClewdrError::PathNotFound(
                    "Failed to get parent directory".to_string(),
                ))?;
                let mut default_config = Config::default();
                let canonical_path = std::fs::canonicalize(config_dir)?;
                println!(
                    "Default config file created at {}/config.toml",
                    canonical_path.display()
                );
                println!("{}", "SET YOUR COOKIE HERE".green());
                default_config.load_from_arg_file();
                default_config = default_config.validate();
                default_config.save()?;
                Ok(default_config)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn load_padtxt(&mut self) {
        let padtxt = self.padtxt_file.clone();
        if padtxt.trim().is_empty() {
            return;
        }

        let Ok(dir) = config_dir() else {
            error!("No config found in cwd or exec dir");
            return;
        };
        let padtxt_path = dir.join(padtxt);
        if !padtxt_path.exists() {
            error!("Pad txt file not found: {}", padtxt_path.display());
            return;
        }
        let Ok(padtxt_string) = std::fs::read_to_string(padtxt_path.clone()) else {
            error!("Failed to read pad txt file: {}", padtxt_path.display());
            return;
        };
        // remove tokenizer special characters
        let re = Regex::new(r"[^\x00-\x7F]").unwrap();
        let tokens = tokenize(&padtxt_string)
            .expect("Failed to tokenize pad txt")
            .into_iter()
            // remove special characters
            .map(|t| re.replace_all(t.1.as_str(), "").trim().to_string())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>();
        if tokens.len() < 4096 {
            panic!("Pad txt file is too short: {}", padtxt_path.display());
        }
        self.pad_tokens = tokens;
    }

    /// API endpoint of server
    pub fn endpoint(&self) -> String {
        if self.rproxy.is_empty() {
            ENDPOINT.to_string()
        } else {
            self.rproxy.clone()
        }
    }

    /// address of proxy
    pub fn address(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }

    /// Save the configuration to a file
    pub fn save(&self) -> Result<(), ClewdrError> {
        // try find existing config file
        let existing = config_dir();
        if let Ok(existing) = existing {
            let config_path = existing.join(CONFIG_NAME);
            // overwrite the file if it exists
            std::fs::write(config_path, toml::ser::to_string_pretty(self)?)?;
            return Ok(());
        }
        // try to create a new config file in exec path or pwd
        let exec_path = std::env::current_exe()?;
        let config_dir = exec_path.parent().ok_or(ClewdrError::PathNotFound(
            "Failed to get parent directory".to_string(),
        ))?;
        // create the config directory if it doesn't exist
        if !config_dir.exists() {
            std::fs::create_dir_all(config_dir)?;
        }
        // Save the config to a file
        let config_path = config_dir.join(CONFIG_NAME);
        let config_string = toml::ser::to_string_pretty(self)?;
        std::fs::write(config_path, config_string)?;
        Ok(())
    }

    /// Validate the configuration
    fn validate(mut self) -> Self {
        if self.password.trim().is_empty() {
            self.password = generate_password(32);
            self.save().expect("Failed to save config");
        }
        self.ip = self.ip.trim().to_string();
        self.rproxy = self.rproxy.trim().to_string();
        self.proxy = self.proxy.trim().to_string();
        let proxy = if self.proxy.is_empty() {
            None
        } else {
            Proxy::all(self.proxy.clone())
                .inspect_err(|e| {
                    error!("Failed to parse proxy: {}", e);
                })
                .ok()
        };
        self.rquest_proxy = proxy;
        self
    }

    /// Load cookies from command line arguments
    fn load_from_arg_file(&mut self) {
        let args: Args = clap::Parser::parse();
        let file = args.cookie_file;
        let Some(file) = file else {
            return;
        };
        let Ok(file_string) = std::fs::read_to_string(file) else {
            return;
        };
        // one line per cookie
        let mut new_array = file_string
            .lines()
            .filter_map(|line| {
                let c = Cookie::from(line);
                if !c.validate() {
                    warn!("Invalid cookie format: {}", line);
                    return None;
                }
                if self.cookie_array.iter().any(|x| x.cookie == c) {
                    warn!("Duplicate cookie: {}", line);
                    return None;
                }
                if self.wasted_cookie.iter().any(|x| x.cookie == c) {
                    warn!("Wasted cookie: {}", line);
                    return None;
                }
                Some(CookieInfo {
                    cookie: c,
                    model: None,
                    reset_time: None,
                })
            })
            .collect::<Vec<_>>();
        // remove duplicates
        new_array.sort_unstable_by(|a, b| a.cookie.cmp(&b.cookie));
        new_array.dedup_by(|a, b| a.cookie == b.cookie);
        self.cookie_array.extend(new_array);
    }
}
