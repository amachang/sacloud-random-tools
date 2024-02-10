use std::{borrow::Borrow, net::Ipv4Addr};
use once_cell::sync::Lazy;
use serde_json::json;

use crate::api::{
    Error,
    Server, ServerId, ServerInfo, ServerPlanId,
    Disk, DiskId, DiskInfo, DiskPlanId, DiskConnection, DiskConfig,
    Appliance, ApplianceId, ApplianceInfo, VpcRouterInfo, VpcRouterPlanId,
    ArchiveId,
    Switch, SwitchId, SwitchInfo,
    SshPublicKey, SshPublicKeyId, SshPublicKeyInfo,
    Note,
    InterfaceDriver,
    Ipv4Net, // SingleLineIpv4Net,
};

static SERVER_PLAN_ID: Lazy<ServerPlanId> = Lazy::new(|| ServerPlanId("100001001".into()));
static DISK_PLAN_ID: Lazy<DiskPlanId> = Lazy::new(|| DiskPlanId(4.into()));


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EquipmentKind {
    PrimaryServer,
    PrimaryServerDisk,
    PrimaryServerSshPublicKey,
    PrimarySwitch,
    PrimaryVpcRouter,
}

impl EquipmentKind {
    pub(crate) fn name(&self, prefix: impl AsRef<str>) -> String {
        match self {
            Self::PrimaryServer => format!("{}-server", prefix.as_ref()),
            Self::PrimaryServerDisk => format!("{}-server", prefix.as_ref()),
            Self::PrimaryServerSshPublicKey => format!("{}-pub-key", prefix.as_ref()),
            Self::PrimarySwitch => format!("{}-switch", prefix.as_ref()),
            Self::PrimaryVpcRouter => format!("{}-vpc-router", prefix.as_ref()),
        }
    }
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
        ssh_public_key_id: impl Borrow<SshPublicKeyId>,
        password: Option<&str>,
    ) -> Result<Self, Error> {
        let prefix = prefix.as_ref();
        let server_id = server_id.borrow();
        let archive_id = archive_id.borrow();
        let ssh_public_key_id = ssh_public_key_id.borrow();
        let name = Self::KIND.name(prefix);

        let note = Note::official_startup_script().await?;

        let info = DiskInfo::builder()
            .name(name.clone())
            .description(name.clone())
            .plan_id(DISK_PLAN_ID.clone())
            .source_archive_id(archive_id.clone())
            .size_mb(20480)
            .connection(DiskConnection::Virtio)
            .server_id(server_id.clone())
            .build();

        let mut config_builder = DiskConfig::builder()
            .host_name(name.clone())
            .ssh_key_ids(vec![ssh_public_key_id.clone()])
            .user_ip_address(Ipv4Addr::new(192, 168, 2, 2))
            .user_subnet(Ipv4Net::new(Ipv4Addr::new(192, 168, 2, 1), 24))
            .change_partition_uuid(false)
            .enable_dhcp(false)
            .note_id_and_variables_pairs(vec![
                (note.id().clone(), json!({ "usacloud": false, "updatepackage": true }))
            ]);

        if let Some(password) = password {
            config_builder = config_builder
                .disable_pw_auth(false)
                .password(password);
        } else {
            config_builder = config_builder
                .disable_pw_auth(true);
        }

        let config = config_builder.build();

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

    pub(crate) async fn update_config(vpc_router_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let vpc_router_id = vpc_router_id.borrow();
        let mut firewall_receive_config = Vec::new();
        let mut firewall_send_config = Vec::new();

        if let Some(local_ip) = public_ip::addr_v4().await {
            firewall_receive_config.push(json!({ "Protocol": "ip", "SourceNetwork": format!("{}/32", local_ip), "Action": "allow", "Description": "local" }));
            firewall_send_config.push(json!({ "Protocol": "ip", "DestinationNetwork": format!("{}/32", local_ip), "Action": "allow", "Description": "local" }));
        }
        firewall_receive_config.extend([
            json!({ "Protocol": "ip", "Action": "deny", "Description": "otherwise" }),
        ]);
        firewall_send_config.extend([
            json!({ "Protocol": "ip", "Action": "deny", "Description": "otherwise" }),
        ]);

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
                                    "Enabled": "True"
                                },
                                "PortForwarding": {
                                    "Config": [ { "Protocol": "tcp", "GlobalPort": "10022", "PrivateAddress": "192.168.2.2", "PrivatePort": "22" } ],
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
}


/* TODO remove old code
async fn create_vpc_router(prefix: impl AsRef<str>) -> Result<(String, Value), Error> {
    let name = vpc_router_name(prefix);
    // InternetConnection の Enabled は "True" など、文字列で指定する。 bool だと BadRequest になる。
    let req_body = json!({
        "Appliance":{
            "Class": "vpcrouter",
            "Name": name.clone(),
            "Description": name.clone(),
            "Plan": { "ID": 1 },
            "Remark":{
                "Router": { "VPCRouterVersion": 2 },
                "Servers": [ {} ],
                "Switch": { "Scope": "shared" },
            },
            "Settings": {
                "Router": {
                    "InternetConnection": { "Enabled": "True" },
                },
            },
        },
    });
    api::request_create_api("appliance", "Appliance", req_body).await
}

async fn update_vpc_router_config(vpc_router_id: impl AsRef<str>) -> Result<(), Error> {
    let vpc_router_id = vpc_router_id.as_ref();

    let mut firewall_receive_config = Vec::new();
    let mut firewall_send_config = Vec::new();

    if let Some(local_ip) = public_ip::addr_v4().await {
        firewall_receive_config.push(json!({ "Protocol": "ip", "SourceNetwork": format!("{}/32", local_ip), "Action": "allow", "Description": "local" }));
        firewall_send_config.push(json!({ "Protocol": "ip", "DestinationNetwork": format!("{}/32", local_ip), "Action": "allow", "Description": "local" }));
    }
    firewall_receive_config.extend([
        json!({ "Protocol": "ip", "Action": "deny", "Description": "otherwise" }),
    ]);
    firewall_send_config.extend([
        json!({ "Protocol": "ip", "Action": "deny", "Description": "otherwise" }),
    ]);

    let req_body = json!({
        "Appliance": {
            "Settings": {
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
                        "Enabled": "True"
                    },
                    "PortForwarding": {
                        "Config": [ { "Protocol": "tcp", "GlobalPort": "10022", "PrivateAddress": "192.168.2.2", "PrivatePort": "22" } ],
                        "Enabled": "True",
                    },
                    "WireGuardServer": {
                        "Config": { "IPAddress": "", "Peers": [] },
                        "Enabled": "False"
                    },
                    "PPTPServer": { "Enabled": "False" },
                    "L2TPIPsecServer": { "Enabled": "False" },
                },
            },
        },
    });
    api::request_update_api(format!("appliance/{}", vpc_router_id), Some(req_body)).await?;

    // appliance に関してはこのコンソール上の「反映」という操作をしないと、設定が反映されない
    api::request_update_api(format!("appliance/{}/config", vpc_router_id), None).await
}
*/
