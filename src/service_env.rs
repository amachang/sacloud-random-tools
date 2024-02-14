use std::{borrow::Borrow, net::{IpAddr, Ipv4Addr}};
use once_cell::sync::Lazy;
use serde_json::json;
use serde::{Serialize, Deserialize};

use crate::api::{
    self,
    Server, ServerId, ServerInfo, ServerPlanId,
    Disk, DiskId, DiskInfo, DiskPlanId, DiskConnection, DiskConfig,
    Appliance, ApplianceId, ApplianceInfo, VpcRouterInfo, VpcRouterPlanId,
    ArchiveId,
    Switch, SwitchId, SwitchInfo,
    SshPublicKey, SshPublicKeyId, SshPublicKeyInfo,
    Note, NoteInfo, NoteId, NoteClass,
    InterfaceDriver,
    Ipv4Net, // SingleLineIpv4Net,
};

static SERVER_PLAN_ID: Lazy<ServerPlanId> = Lazy::new(|| ServerPlanId("100001001".into()));
static DISK_PLAN_ID: Lazy<DiskPlanId> = Lazy::new(|| DiskPlanId(4.into()));

pub(crate) const PRIMARY_SERVER_FORWARDED_PORT: u16 = 10022;
const CONFIG_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config/config.json"));
const SETUP_SHELL_NOTE_CONTENT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/note/setup.sh"));

pub(crate) static CONFIG: Lazy<Config> = Lazy::new(|| { Config::default() });

#[derive(Debug, Serialize)]
pub(crate) enum Error {
    ApiError(api::Error),
}

impl From<api::Error> for Error {
    fn from(e: api::Error) -> Self {
        Self::ApiError(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EquipmentKind {
    PrimaryServer,
    PrimaryServerDisk,
    PrimaryServerSshPublicKey,
    PrimarySwitch,
    PrimaryVpcRouter,
    PrimaryServerSetupShellNote,
}

impl EquipmentKind {
    pub(crate) fn name(&self, prefix: impl AsRef<str>) -> String {
        match self {
            Self::PrimaryServer => format!("{}-server", prefix.as_ref()),
            Self::PrimaryServerDisk => format!("{}-server", prefix.as_ref()),
            Self::PrimaryServerSshPublicKey => format!("{}-pub-key", prefix.as_ref()),
            Self::PrimarySwitch => format!("{}-switch", prefix.as_ref()),
            Self::PrimaryVpcRouter => format!("{}-vpc-router", prefix.as_ref()),
            Self::PrimaryServerSetupShellNote => format!("{}-server-setup-shell", prefix.as_ref()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde()]
    pub(crate) server: ServerConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ServerConfig {
    #[serde()]
    wireguard: WireGuardConfig,
}

impl Default for Config {
    fn default() -> Self {
        serde_json::from_str(CONFIG_JSON).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireGuardConfig {
    #[serde()]
    interface: WireGuardInterfaceConfig,

    #[serde()]
    peer: WireGuardPeerConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireGuardInterfaceConfig {
    #[serde()]
    private_key: String,

    // 雑
    #[serde()]
    address: Vec<String>,

    #[serde()]
    dns: Vec<IpAddr>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireGuardPeerConfig {
    #[serde()]
    public_key: String,

    #[serde()]
    endpoint: IpAddr,
}

#[derive(Debug)]
pub(crate) struct PrimaryServer {
    server: Server,
}

impl PrimaryServer {
    const KIND: EquipmentKind = EquipmentKind::PrimaryServer;

    pub(crate) async fn try_get(prefix: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let server = Server::get_by_name(&name).await?;
        Ok(server.map(|server| Self { server }))
    }

    pub(crate) async fn create(prefix: impl AsRef<str>, switch_id: impl Borrow<SwitchId>) -> Result<Self, Error> {
        let prefix = prefix.as_ref();
        let switch_id = switch_id.borrow();
        let name = Self::KIND.name(prefix);

        let server_info = ServerInfo::builder()
            .name(name.clone())
            .server_plan(SERVER_PLAN_ID.clone())
            .description(name.clone())
            .host_name(name.clone())
            .connected_switch_ids(vec![switch_id.clone()])
            .interface_driver(InterfaceDriver::Virtio)
            .wait_disk_migration(true)
            .build();

        let server = Server::create(server_info).await?;

        Ok(Self { server })
    }

    pub(crate) fn id(&self) -> &ServerId {
        self.server.id()
    }

}

#[derive(Debug)]
pub(crate) struct PrimaryServerDisk {
    disk: Disk,
}

impl PrimaryServerDisk {
    const KIND: EquipmentKind = EquipmentKind::PrimaryServerDisk;

    pub(crate) async fn try_get(prefix: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let disk = Disk::get_by_name(&name).await?;
        Ok(disk.map(|disk| Self { disk }))
    }

    pub(crate) async fn create_for_server(
        prefix: impl AsRef<str>,
        server_id: impl Borrow<ServerId>,
        archive_id: impl Borrow<ArchiveId>,
        startup_shell_note_id: impl Borrow<NoteId>,
        ssh_public_key_id: impl Borrow<SshPublicKeyId>,
    ) -> Result<Self, Error> {
        let prefix = prefix.as_ref();
        let server_id = server_id.borrow();
        let archive_id = archive_id.borrow();
        let startup_shell_note_id = startup_shell_note_id.borrow();
        let ssh_public_key_id = ssh_public_key_id.borrow();
        let name = Self::KIND.name(prefix);

        let info = DiskInfo::builder()
            .name(name.clone())
            .description(name.clone())
            .plan_id(DISK_PLAN_ID.clone())
            .source_archive_id(archive_id.clone())
            .size_mb(20480)
            .connection(DiskConnection::Virtio)
            .server_id(server_id.clone())
            .build();

        let config = DiskConfig::builder()
            .host_name(name.clone())
            .ssh_key_ids(vec![ssh_public_key_id.clone()])
            .user_ip_address(Ipv4Addr::new(192, 168, 2, 2))
            .user_subnet(Ipv4Net::new(Ipv4Addr::new(192, 168, 2, 1), 24))
            .change_partition_uuid(false)
            .enable_dhcp(false)
            .disable_pw_auth(true)
            .setup_shell_note(startup_shell_note_id.clone(), json!({}))
            .build();

        let disk = Disk::create(info, config).await?;

        Ok(Self { disk })

    }

    pub(crate) fn id(&self) -> &DiskId {
        self.disk.id()
    }
}


#[derive(Debug)]
pub(crate) struct PrimaryServerSshPublicKey {
    ssh_public_key: SshPublicKey,
}

impl PrimaryServerSshPublicKey {
    const KIND: EquipmentKind = EquipmentKind::PrimaryServerSshPublicKey;

    pub(crate) async fn try_get(prefix: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let prefix = prefix.as_ref();
        let kind = EquipmentKind::PrimaryServerSshPublicKey;
        let name = kind.name(prefix);

        let ssh_public_key = SshPublicKey::get_by_name(&name).await?;
        Ok(ssh_public_key.map(|ssh_public_key| Self { ssh_public_key }))
    }

    pub(crate) async fn create(prefix: impl AsRef<str>, public_key: impl AsRef<str>) -> Result<Self, Error> {
        let name = Self::KIND.name(prefix);
        let info = SshPublicKeyInfo::builder()
            .name(name.clone())
            .description(name.clone())
            .public_key(public_key.as_ref())
            .build();
        let ssh_public_key = SshPublicKey::create(info).await?;
        Ok(Self { ssh_public_key })
    }

    pub(crate) fn id(&self) -> &SshPublicKeyId {
        self.ssh_public_key.id()
    }

    pub(crate) fn public_key(&self) -> &str {
        self.ssh_public_key.public_key()
    }
}


#[derive(Debug)]
pub(crate) struct PrimarySwitch {
    switch: Switch,
}

impl PrimarySwitch {
    const KIND: EquipmentKind = EquipmentKind::PrimarySwitch;

    pub(crate) async fn try_get(prefix: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let switch = Switch::get_by_name(&name).await?;
        Ok(switch.map(|switch| Self { switch }))
    }

    pub(crate) async fn create(prefix: impl AsRef<str>) -> Result<Self, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let info = SwitchInfo::builder()
            .name(name.clone())
            .description(name.clone())
            .build();
        let switch = Switch::create(info).await?;
        Ok(Self { switch })
    }

    pub(crate) fn id(&self) -> &SwitchId {
        self.switch.id()
    }
}


#[derive(Debug)]
pub(crate) struct PrimaryVpcRouter {
    appliance: Appliance,
}

impl PrimaryVpcRouter {
    const KIND: EquipmentKind = EquipmentKind::PrimaryVpcRouter;

    pub(crate) async fn try_get(prefix: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let appliance = Appliance::get_by_name(&name).await?;
        Ok(appliance.map(|appliance| Self { appliance }))
    }

    pub(crate) async fn create(prefix: impl AsRef<str>) -> Result<Self, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let info = ApplianceInfo::builder()
            .name(name.clone())
            .description(name.clone())
            .vpc_router(
                VpcRouterInfo::builder()
                    .plan_id(VpcRouterPlanId::new(1))
                    .remark(
                        json!({
                            "Router": { "VPCRouterVersion": 2 },
                            "Servers": [ {} ],
                            "Switch": { "Scope": "shared" },
                        })
                    )
                    .settings(
                        json!({
                            "Router": {
                                "InternetConnection": { "Enabled": "True" },
                            },
                        })
                    )
                    .build()
            )
            .build();
        let appliance = Appliance::create(info).await?;

        Ok(Self { appliance })
    }

    pub(crate) async fn update_config(vpc_router_id: impl Borrow<ApplianceId>, firewall_enabled: bool) -> Result<(), Error> {
        let vpc_router_id = vpc_router_id.borrow();
        let mut firewall_receive_config = Vec::new();
        let mut firewall_send_config = Vec::new();

        if let Some(local_ip) = public_ip::addr_v4().await {
            firewall_receive_config.push(json!({ "Protocol": "ip", "SourceNetwork": format!("{}/32", local_ip), "Action": "allow", "Description": "local" }));
            firewall_send_config.push(json!({ "Protocol": "ip", "DestinationNetwork": format!("{}/32", local_ip), "Action": "allow", "Description": "local" }));
        }

        let wireguard_peer_endpoint_ip = CONFIG.server.wireguard.peer.endpoint;
        firewall_send_config.push(json!({ "Protocol": "udp", "DestinationNetwork": format!("{}/32", wireguard_peer_endpoint_ip), "DestinationPort": "51820", "Action": "allow", "Description": "wireguard" }));

        firewall_receive_config.push(json!({ "Protocol": "ip", "Action": "deny", "Description": "otherwise" }));
        firewall_send_config.push(json!({ "Protocol": "ip", "Action": "deny", "Description": "otherwise" }));

        let info = ApplianceInfo::builder()
            .vpc_router_info(
                VpcRouterInfo::builder()
                    .settings(
                        json!({
                            "Router": {
                                "Interfaces": [
                                    null,
                                    { "IPAddress": [ "192.168.2.1" ], "NetworkMaskLen": 24 },
                                ],
                                "Firewall": {
                                    "Config": [
                                    {
                                        "Receive": firewall_receive_config,
                                        "Send": firewall_send_config,
                                    },
                                    ],
                                    "Enabled": if firewall_enabled { "True" } else { "False" },
                                },
                                "PortForwarding": {
                                    "Config": [ { "Protocol": "tcp", "GlobalPort": PRIMARY_SERVER_FORWARDED_PORT.to_string(), "PrivateAddress": "192.168.2.2", "PrivatePort": "22" } ],
                                    "Enabled": "True",
                                },
                                "WireGuardServer": {
                                    "Config": { "IPAddress": "", "Peers": [] },
                                    "Enabled": "False"
                                },
                                "PPTPServer": { "Enabled": "False" },
                                "L2TPIPsecServer": { "Enabled": "False" },
                            }
                        })
                )
                .build()
            )
            .build();

        Appliance::update(vpc_router_id, info).await?;
        Ok(())
    }

    pub(crate) fn id(&self) -> &ApplianceId {
        self.appliance.id()
    }

    pub(crate) fn public_shared_ip(&self) -> Result<Ipv4Addr, Error> {
        let ip = self.appliance.public_shared_ip()?;
        Ok(ip)
    }
}

#[derive(Debug)]
pub(crate) struct PrimaryServerSetupShellNote {
    note: Note,
}

impl PrimaryServerSetupShellNote {
    const KIND: EquipmentKind = EquipmentKind::PrimaryServerSetupShellNote;

    pub(crate) async fn try_get(prefix: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let note = Note::get_by_name(&name).await?;
        Ok(note.map(|note| Self { note }))
    }
    
    pub(crate) async fn update_content_if_needed(id: impl Borrow<NoteId>) -> Result<(), Error> {
        let id = id.borrow();
        let note = Note::get(id).await?;
        if note.content() == SETUP_SHELL_NOTE_CONTENT {
            return Ok(());
        }

        let info = NoteInfo::builder()
            .content(SETUP_SHELL_NOTE_CONTENT)
            .build();
        Note::update(id, info).await?;
        Ok(())
    }

    pub(crate) async fn create(prefix: impl AsRef<str>) -> Result<Self, Error> {
        let prefix = prefix.as_ref();
        let name = Self::KIND.name(prefix);

        let info = NoteInfo::builder()
            .name(name.clone())
            .class(NoteClass::Shell)
            .description(name.clone())
            .content(SETUP_SHELL_NOTE_CONTENT)
            .build();

        let note = Note::create(info).await?;
        Ok(Self { note })
    }

    pub(crate) fn id(&self) -> &NoteId {
        &self.note.id()
    }
}

