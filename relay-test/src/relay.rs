use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use hyper::http::HeaderName;
use no_deadlocks::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::http::HeaderMap;
use axum::response::Json;
use chrono::Utc;
use lazy_static::lazy_static;
use relay_auth::{
    PublicKey, RegisterChallenge, RegisterResponse, RelayVersion, SecretKey, SignedRegisterState,
};
use relay_base_schema::project::{ProjectId, ProjectKey};
use relay_common::Scheme;
use relay_config::Config;
use relay_config::Credentials;
use relay_config::RelayInfo;
use relay_config::UpstreamDescriptor;
use relay_sampling::config::{RuleType, SamplingRule};
use relay_system::{channel, Addr, Interface};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::future::Future;
use tokio::runtime::Runtime;
use tokio::sync::{oneshot, Mutex as TokioMutex};
use tokio::task::JoinHandle;
use uuid::fmt::Simple; // It's better to use Tokio's Mutex in async contexts.
use uuid::Uuid;

use crate::consumers::processing_config;
use crate::mini_sentry::{MiniSentry, MiniSentryInner};
use crate::test_envelopy::RawEnvelope;
use crate::{
    envelope_to_request, merge, outcomes_enabled_config, random_port, BackgroundProcess, ConfigDir,
    EnvelopeBuilder, DEFAULT_DSN_PUBLIC_KEY,
};

pub trait Upstream {
    fn url(&self) -> String;
    fn internal_error_dsn(&self) -> String;
    fn insert_known_relay(&self, relay_id: Uuid, public_key: PublicKey);
}

impl Upstream for Relay {
    fn url(&self) -> String {
        self.url()
    }

    fn internal_error_dsn(&self) -> String {
        self.upstream_dsn.clone()
    }

    fn insert_known_relay(&self, relay_id: Uuid, public_key: PublicKey) {
        // idk man
    }
}

impl Upstream for MiniSentry {
    fn url(&self) -> String {
        self.inner.lock().unwrap().url()
    }

    fn internal_error_dsn(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .internal_error_dsn()
    }

    fn insert_known_relay(&self, relay_id: Uuid, public_key: PublicKey) {
        self.inner.lock().unwrap().known_relays.insert(
            relay_id,
            RelayInfo {
                public_key,
                internal: true,
            },
        );
    }
}

fn default_opts(url: String, internal_error_dsn: String, port: u16, host: String) -> Value {
    json!({
        "relay": {
            "upstream": url,
            "host": host,
            "port": port,
            "tls_port": null,
            "tls_private_key": null,
            "tls_cert": null,
        },
        "sentry": {
            "dsn": internal_error_dsn,
            "enabled": true,
        },
        "limits": {
            "max_api_file_upload_size": "1MiB",
        },
        "cache": {
            "batch_interval": 0,
        },
        "logging": {
            "level": "trace",
        },
        "http": {
            "timeout": 3,
        },
        "processing": {
            "enabled": false,
            "kafka_config": [],
            "redis": "",
        },
        "outcomes": {
            "aggregator": {
                "bucket_interval": 1,
                "flush_interval": 0,
            },
        },
    })
}

pub struct Relay {
    server_address: SocketAddr,
    process: BackgroundProcess,
    relay_id: Uuid,
    secret_key: SecretKey,
    health_check_passed: bool,
    config: Arc<Config>,
    client: reqwest::Client,
    upstream_dsn: String,
}

impl Relay {}

pub struct RelayBuilder<'a, U: Upstream> {
    pub config: serde_json::Value,
    mini_version: Option<RelayVersion>,
    upstream: &'a U,
}

impl<'a, U: Upstream> RelayBuilder<'a, U> {
    pub fn enable_processing(mut self) -> Self {
        let proc = json!( {"processing": {
            "enabled": true,
            "kafka_config": [],
            "redis": "redis://127.0.0.1",
        }});

        let proc = processing_config();

        self.config = merge(self.config, proc, vec![]);
        self
    }

    pub fn set_min_version(mut self, version: RelayVersion) -> Self {
        self.mini_version = Some(version);
        self
    }

    pub fn set_accept_unknown_items(mut self, val: bool) -> Self {
        let val = json!({
            "routing": {
                "accept_unknown_items": val,
            }
        });

        self.config = merge(self.config, val, vec![]);
        self
    }

    pub fn merge_config(mut self, value: serde_json::Value) -> Self {
        self.config = merge(self.config, value, vec![]);
        self
    }

    pub fn enable_outcomes(mut self) -> Self {
        self.config = merge(self.config, outcomes_enabled_config(), vec![]);
        self
    }

    pub fn build(self) -> Relay {
        dbg!();
        let config = Config::from_json_value(self.config).unwrap();
        let version = &self.mini_version;
        let relay_bin = get_relay_binary().unwrap();
        dbg!();

        let mut dir = ConfigDir::new();
        let dir = dbg!(dir.create("relay"));

        let credentials = Relay::load_credentials(&config, &dir);
        dbg!();

        self.upstream
            .insert_known_relay(credentials.id, credentials.public_key);
        dbg!();

        let process = BackgroundProcess::new(
            relay_bin.as_path().to_str().unwrap(),
            &["-c", dir.as_path().to_str().unwrap(), "run"],
        );
        dbg!();

        let server_address = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            config.values.relay.port,
        );
        dbg!();

        // We need this delay before we start sending to relay.
        std::thread::sleep(Duration::from_secs(1));
        dbg!();

        Relay {
            process,
            relay_id: credentials.id,
            secret_key: credentials.secret_key,
            server_address,
            health_check_passed: true,
            config: Arc::new(config),
            client: reqwest::Client::new(),
            upstream_dsn: self.upstream.internal_error_dsn(),
        }
    }
}

impl<'a> Relay {
    fn descriptor(&'a self, host: &'a str) -> UpstreamDescriptor<'a> {
        UpstreamDescriptor::new(host, self.config.values.relay.port, Scheme::Http)
    }

    pub fn server_address(&self) -> SocketAddr {
        self.server_address
    }

    pub fn get_dsn(&self, public_key: ProjectKey) -> String {
        let x = self.server_address();
        let host = x.ip();
        let port = x.port();

        format!("http://{public_key}:@{host}:{port}/42")
    }

    fn load_credentials(config: &Config, relay_dir: &Path) -> Credentials {
        dbg!(&relay_dir);
        let relay_bin = get_relay_binary().unwrap();
        let config_path = relay_dir.join("config.yml");

        std::fs::write(
            config_path.as_path(),
            serde_yaml::to_string(&config.values).unwrap(),
        )
        .unwrap();

        dbg!(&relay_bin);
        dbg!(&config_path.parent());
        let output = std::process::Command::new(relay_bin.as_path())
            .arg("-c")
            .arg(config_path.parent().unwrap())
            .arg("credentials")
            .arg("generate")
            .output()
            .unwrap();

        if !output.status.success() {
            dbg!(&output);
            panic!("Command execution failed");
        }
        let credentials_path = relay_dir.join("credentials.json");

        let credentials_str = std::fs::read_to_string(credentials_path).unwrap();
        serde_json::from_str(&credentials_str).expect("Failed to parse JSON")
    }

    pub fn new<U: Upstream + 'static>(upstream: &U) -> Self {
        Self::builder(upstream).build()
    }

    pub fn builder<U: Upstream + 'static>(upstream: &U) -> RelayBuilder<U> {
        let host = "127.0.0.1".into();
        let port = random_port();
        let url = upstream.url();
        let internal_error_dsn = upstream.internal_error_dsn();

        let config = default_opts(dbg!(url), dbg!(internal_error_dsn), port, host);

        RelayBuilder {
            config,
            upstream,
            mini_version: None,
        }
    }

    fn url(&self) -> String {
        format!(
            "http://{}:{}",
            self.server_address.ip().to_string(),
            self.server_address.port()
        )
    }

    pub fn get_auth_header(&self, dsn_key: ProjectKey) -> String {
        format!(
            "Sentry sentry_version=5, sentry_timestamp=1535376240291, sentry_client=rust-node/2.6.3, sentry_key={}",
            dsn_key
        )
    }

    pub fn envelope_url(&self, project_id: ProjectId) -> String {
        let endpoint = format!("/api/{}/envelope/", project_id);
        format!("{}{}", self.url(), endpoint)
    }

    pub fn send_envelope_to_url(&self, envelope: RawEnvelope, url: &str) -> Response {
        use reqwest::header::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-sentry-envelope"),
        );
        headers.insert(
            "X-Sentry-Auth",
            HeaderValue::from_str(&self.get_auth_header(envelope.dsn_public_key)).unwrap(),
        );

        let data = envelope.serialize();

        // Add additional headers from envelope if necessary
        for (key, value) in envelope.http_headers.iter() {
            headers.insert(
                HeaderName::from_bytes(key.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }

        dbg!("sending envelope!");
        dbg!(&url, &headers, &data);
        Runtime::new().unwrap().block_on(async {
            dbg!(
                reqwest::Client::new()
                    .post(url)
                    .headers(headers)
                    .body(data)
                    .send()
                    .await
            )
            .unwrap()
        })
    }

    pub fn send_envelope(&self, envelope: RawEnvelope) {
        let url = self.envelope_url(envelope.project_id);
        self.send_envelope_to_url(envelope, &url);
    }
}
use reqwest::{self, Response};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

fn get_relay_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let version = "latest";
    if version == "latest" {
        return Ok(std::env::var("RELAY_BIN")
            .map_or_else(|_| "../target/debug/relay".into(), PathBuf::from)
            .canonicalize()
            .expect("Failed to get absolute path"));
    };

    let filename = match env::consts::OS {
        "linux" => "relay-Linux-x86_64",
        "macos" => "relay-Darwin-x86_64",
        "windows" => "relay-Windows-x86_64.exe",
        _ => panic!("Unsupported OS"),
    };

    let download_path = PathBuf::from(format!(
        "target/relay_releases_cache/{}_{}",
        filename, version
    ));

    if !Path::new(&download_path).exists() {
        let download_url = format!(
            "https://github.com/getsentry/relay/releases/download/{}/{}",
            version, filename
        );

        let client = reqwest::blocking::Client::new();
        let mut request = client.get(download_url);

        if let Ok(token) = env::var("GITHUB_TOKEN") {
            request = request.bearer_auth(token);
        }

        let response = request.send()?.error_for_status()?;

        // Adjusted part: Read the entire response body at once.
        let content = response.bytes()?;

        fs::create_dir_all(Path::new(&download_path).parent().unwrap())?;
        let mut file = File::create(&download_path)?;

        // Write the entire content to the file.
        file.write_all(&content)?;

        let mut perms = fs::metadata(&download_path)?.permissions();
        perms.set_mode(0o700); // UNIX-specific; for Windows, you'll need a different approach
        fs::set_permissions(&download_path, perms)?;
    }

    Ok(download_path)
}

fn get_auth_header() -> String {
    let dsn_key = DEFAULT_DSN_PUBLIC_KEY;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    let client_name = "my-rust-client/1.0.0";
    let sentry_version = "7"; // or "5", depending on your Sentry server version

    format!(
        "Sentry sentry_version={},sentry_timestamp={},sentry_client={},sentry_key={}",
        sentry_version, timestamp, client_name, dsn_key
    )
}
