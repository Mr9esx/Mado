use mdns_sd::{ServiceDaemon, ServiceEvent};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose,
};
use rustls::{
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    ClientConfig, ClientConnection, RootCertStore, StreamOwned,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const SERVICE: &str = "_mado._tcp.local.";
const PORT: u16 = 47861;
const SERVER_NAME: &str = "mado.local";
const NETWORK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    ca: Vec<u8>,
    certificate: Vec<u8>,
    key: Vec<u8>,
}

pub struct Provisioning {
    ca: Certificate,
    certificate: Certificate,
    key: KeyPair,
    pub client: Credentials,
}

impl Provisioning {
    pub fn new() -> Result<Self, String> {
        Self::issue().map_err(|error| format!("无法生成设备连接证书：{error}"))
    }

    fn issue() -> Result<Self, rcgen::Error> {
        let mut ca_params = CertificateParams::default();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Mado Device CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let ca_key = KeyPair::generate()?;
        let ca = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params, ca_key);

        let mut server = CertificateParams::new(vec![SERVER_NAME.to_owned()])?;
        server
            .distinguished_name
            .push(DnType::CommonName, SERVER_NAME);
        server.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let key = KeyPair::generate()?;
        let certificate = server.signed_by(&key, &issuer)?;

        let mut client = CertificateParams::default();
        client
            .distinguished_name
            .push(DnType::CommonName, "Mado Studio");
        client.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        client.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_key = KeyPair::generate()?;
        let client_cert = client.signed_by(&client_key, &issuer)?;
        let client = Credentials {
            ca: ca.der().to_vec(),
            certificate: client_cert.der().to_vec(),
            key: client_key.serialize_der(),
        };
        Ok(Self {
            ca,
            certificate,
            key,
            client,
        })
    }

    pub fn request(&self, id: Uuid, ssid: String, password: String) -> Value {
        json!({
            "requestId": id,
            "ssid": ssid,
            "password": password,
            "ca": self.ca.pem(),
            "certificate": self.certificate.pem(),
            "key": self.key.serialize_pem(),
        })
    }
}

#[derive(Default, Deserialize, Serialize)]
pub struct Stored {
    pub active: Option<Credentials>,
    pub pending: Option<Credentials>,
    #[serde(default)]
    pub pending_id: Option<Uuid>,
}

impl Stored {
    pub fn load(directory: &Path, device_id: &str) -> Result<Self, String> {
        if !valid_id(device_id) {
            return Err("设备标识无效。".into());
        }
        let path = directory.join(format!("{device_id}.json"));
        match fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "设备的 Wi-Fi 配对记录无法读取。".into())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn save(&self, directory: &Path, device_id: &str) -> Result<(), String> {
        if !valid_id(device_id) {
            return Err("设备标识无效。".into());
        }
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temp = directory.join(format!(".{}.tmp", Uuid::new_v4()));
        let path = directory.join(format!("{device_id}.json"));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> io::Result<()> {
            let mut file = options.open(&temp)?;
            serde_json::to_writer(&mut file, self)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result.map_err(|error| format!("无法保存 Wi-Fi 配对记录：{error}"))
    }

    pub fn reconcile(&mut self, network_saved: bool, saved_id: Option<Uuid>) {
        if network_saved
            && (saved_id.is_some() && self.pending_id == saved_id
                || saved_id.is_none() && self.pending_id.is_none() && self.active.is_none())
        {
            if let Some(pending) = self.pending.take() {
                self.active = Some(pending);
            }
            self.pending_id = None;
        }
    }

    pub fn candidates(self) -> Vec<Credentials> {
        self.pending.into_iter().chain(self.active).collect()
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn discover(
    ids: &BTreeSet<String>,
    seconds: u64,
    stop_on_match: bool,
) -> BTreeMap<String, BTreeSet<Ipv4Addr>> {
    let mut found = BTreeMap::<String, BTreeSet<Ipv4Addr>>::new();
    if ids.is_empty() {
        return found;
    }
    let Ok(daemon) = ServiceDaemon::new() else {
        return found;
    };
    if let Ok(events) = daemon.browse(SERVICE) {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while let Ok(event) = events.recv_deadline(deadline) {
            if let ServiceEvent::ServiceResolved(service) = event {
                if service.port != PORT {
                    continue;
                }
                if let Some(id) = service
                    .get_property_val_str("id")
                    .filter(|id| ids.contains(*id))
                {
                    let addresses = found.entry(id.to_owned()).or_default();
                    addresses.extend(service.get_addresses_v4());
                    if stop_on_match && !addresses.is_empty() {
                        break;
                    }
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let _ = daemon.stop_browse(SERVICE);
    }
    let _ = daemon.shutdown();
    found
}

#[derive(Serialize)]
pub struct Choice {
    id: String,
    name: String,
    address: Option<Ipv4Addr>,
}

pub fn scan(directory: &Path) -> Result<Vec<Choice>, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let ids: BTreeSet<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let id = name.to_str()?.strip_suffix(".json")?;
            (valid_id(id)
                && Stored::load(directory, id).is_ok_and(|stored| !stored.candidates().is_empty()))
            .then(|| id.to_owned())
        })
        .collect();
    let addresses = discover(&ids, 2, false);
    Ok(ids
        .into_iter()
        .map(|id| Choice {
            address: addresses
                .get(&id)
                .and_then(|values| values.first().copied()),
            name: format!("Mado-{}", id[8..].to_uppercase()),
            id,
        })
        .collect())
}

pub type WifiIo = StreamOwned<ClientConnection, TcpStream>;

fn client_config(credentials: &Credentials) -> Result<ClientConfig, String> {
    let mut roots = RootCertStore::empty();
    roots
        .add(credentials.ca.clone().into())
        .map_err(|error| error.to_string())?;
    ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .with_root_certificates(roots)
        .with_client_auth_cert(
            vec![credentials.certificate.clone().into()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(credentials.key.clone())),
        )
        .map_err(|error| error.to_string())
}

fn open_socket(address: SocketAddr, credentials: &Credentials) -> Result<WifiIo, String> {
    let config = client_config(credentials)?;
    let name = ServerName::try_from(SERVER_NAME).map_err(|error| error.to_string())?;
    let mut client =
        ClientConnection::new(Arc::new(config), name).map_err(|error| error.to_string())?;
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_millis(1600))
        .map_err(|error| error.to_string())?;
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + NETWORK_TIMEOUT;
    while client.is_handshaking() || client.wants_write() {
        match client.complete_io(&mut socket) {
            Ok(_) => {}
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) => return Err(error.to_string()),
        }
        if Instant::now() >= deadline {
            return Err("Wi-Fi 握手超时。".into());
        }
    }
    socket
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(120)))
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(Some(NETWORK_TIMEOUT))
        .map_err(|error| error.to_string())?;
    socket
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    Ok(StreamOwned::new(client, socket))
}

pub struct Target {
    pub id: String,
    pub address: Option<Ipv4Addr>,
    pub credentials: Vec<Credentials>,
}

impl Target {
    pub fn open(&self) -> Result<WifiIo, String> {
        let mut error =
            "局域网内未找到设备，请确认设备和电脑在同一网络，或填写设备 IP。".to_owned();
        let mut try_at = |ip: Ipv4Addr| {
            for credential in &self.credentials {
                match open_socket(SocketAddr::from((ip, PORT)), credential) {
                    Ok(connection) => return Some(connection),
                    Err(reason) => error = format!("无法通过 Wi-Fi 安全连接设备：{reason}"),
                }
            }
            None
        };
        if let Some(connection) = self.address.and_then(&mut try_at) {
            return Ok(connection);
        }
        let ids = BTreeSet::from([self.id.clone()]);
        if let Some(addresses) = discover(&ids, 2, true).get(&self.id) {
            for &address in addresses {
                if Some(address) != self.address {
                    if let Some(connection) = try_at(address) {
                        return Ok(connection);
                    }
                }
            }
        }
        Err(error)
    }
}

pub fn verify_at(address: Ipv4Addr, credentials: &Credentials) -> Result<(), String> {
    open_socket(SocketAddr::from((address, PORT)), credentials).map(|_| ())
}

#[cfg(test)]
#[path = "wifi_display_tests.rs"]
mod tests;
