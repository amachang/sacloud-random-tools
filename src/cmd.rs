use std::{path::PathBuf, io};
use clap::{Parser, Subcommand};
use tokio::fs;

use crate::{
    api::{
        self,
        Server, ServerId,
        Switch, SwitchId,
        SshPublicKeyId,
        Appliance, ApplianceId,
        Archive,
        Disk,
    },
    service_env::{
        PrimaryVpcRouter,
        PrimarySwitch,
        PrimaryServer,
        PrimaryServerDisk,
        PrimaryServerSshPublicKey,
    },
};

#[derive(Debug)]
pub(crate) enum Error {
    PrimaryServerNotConnectedToSwitch(ServerId, SwitchId),
    PrimarySwitchNotConnectedToVpcRouter(SwitchId, ApplianceId),
    PrimarySshPublicKeyAlreadyRegisteredButMismatch(SshPublicKeyId, String, String),
    PrimarySshPublicKeyNotGivenForNewServerDisk,
    PrimarySshPublicKeyGivenButCouldntRead(PathBuf, io::Error),
    ApiError(api::Error),
}

impl From<api::Error> for Error {
    fn from(e: api::Error) -> Self {
        Error::ApiError(e)
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum Cmd {
    Update(UpdateCmd),
}

impl Cmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        match self {
            Cmd::Update(cmd) => cmd.run().await,
        }
    }
}

#[derive(Debug, Parser)]
pub(crate) struct UpdateCmd {
    #[arg(long)]
    prefix: String,

    #[arg(long)]
    pubkey: Option<PathBuf>,

    #[arg(long)]
    password: Option<String>,
}

impl UpdateCmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        let prefix = self.prefix.as_str();
        let ssh_public_key = if let Some(ssh_public_key_path) = &self.pubkey {
            match fs::read_to_string(ssh_public_key_path).await {
                Ok(ssh_public_key) => Some(ssh_public_key),
                Err(e) => return Err(Error::PrimarySshPublicKeyGivenButCouldntRead(ssh_public_key_path.clone(), e)),
            }
        } else {
            None
        };

        let password = self.password.as_deref();

        // VPC Router
        let vpc_router = if let Some(vpc_router) = PrimaryVpcRouter::try_get(prefix).await? {
            vpc_router
        } else {
            let vpc_router = PrimaryVpcRouter::create(prefix).await?;
            vpc_router
        };
        Appliance::wait_available(vpc_router.id()).await?;

        // Switch
        let switch = if let Some(switch) = PrimarySwitch::try_get(prefix).await? {
            let is_connected = Appliance::is_connected_to_switch(vpc_router.id(), switch.id()).await?;
            if !is_connected {
                return Err(Error::PrimarySwitchNotConnectedToVpcRouter(switch.id().clone(), vpc_router.id().clone()))
            }
            switch
        } else {
            let switch = PrimarySwitch::create(prefix).await?;
            Appliance::connect_to_switch(vpc_router.id(), switch.id()).await?;
            switch
        };
        Switch::wait_available(switch.id()).await?;

        Appliance::up(vpc_router.id()).await?;
        Appliance::wait_up(vpc_router.id()).await?;
        Appliance::wait_available(vpc_router.id()).await?;

        // Server
        let server = if let Some(server) = PrimaryServer::try_get(prefix).await? {
            let is_connected = Server::is_connected_to_switch(server.id(), switch.id()).await?;
            if !is_connected {
                return Err(Error::PrimaryServerNotConnectedToSwitch(server.id().clone(), switch.id().clone()))
            }
            server
        } else {
            let server = PrimaryServer::create(prefix, switch.id()).await?;
            server
        };

        // Disk
        let disk = if let Some(disk) = PrimaryServerDisk::try_get(prefix).await? {
            disk
        } else {
            let ssh_public_key = if let Some(current_ssh_public_key) = PrimaryServerSshPublicKey::try_get(prefix).await? {
                if let Some(ssh_public_key) = ssh_public_key {
                    if current_ssh_public_key.public_key() != ssh_public_key {
                        // 同名の古い公開鍵を消していいのかわからないのでエラーにする
                        return Err(Error::PrimarySshPublicKeyAlreadyRegisteredButMismatch(
                                current_ssh_public_key.id().clone(),
                                current_ssh_public_key.public_key().to_string(),
                                ssh_public_key.to_string(),
                        ));
                    }
                }
                current_ssh_public_key
            } else {
                let Some(ssh_public_key) = ssh_public_key else {
                    return Err(Error::PrimarySshPublicKeyNotGivenForNewServerDisk);
                };
                let ssh_public_key = PrimaryServerSshPublicKey::create(prefix, ssh_public_key).await?;
                ssh_public_key
            };
            let archive = Archive::latest_public_ubuntu().await?;
            let disk = PrimaryServerDisk::create_for_server(prefix, server.id(), archive.id(), ssh_public_key.id(), password).await?;
            disk
        };
        Disk::wait_available(disk.id()).await?;
        Server::wait_available(server.id()).await?;

        Server::up(server.id()).await?;
        Server::wait_up(server.id()).await?;
        Server::wait_available(server.id()).await?;

        PrimaryVpcRouter::update_config(vpc_router.id()).await?;
        Appliance::apply_config(vpc_router.id()).await?;
        Appliance::wait_available(vpc_router.id()).await?;
        Ok(())
    }
}

/* TODO remove old code
pub(crate) async fn show_all_resources() -> Result<(), Error> {
    let resource_pairs = vec![
        ("privatehost", "PrivateHosts"),
        ("server", "Servers"),
        ("disk", "Disks"),
        ("switch", "Switches"),
        ("archive", "Archives"),
        ("cdrom", "CDROMs"),
        ("bridge", "Bridges"),
        ("internet", "Internet"),
        // ("ipaddress", "IPAddress"),
        // ("ipv6addr", "IPv6Addrs"),
        // ("ipv6net", "IPv6Nets"),
        // ("subnet", "Subnets"),
        // ("interface", "Interfaces"),
        ("packetfilter", "PacketFilters"),
        ("appliance", "Appliances"),
        ("commonserviceitem", "CommonServiceItems"),
        ("icon", "Icons"),
        ("note", "Notes"),
        ("sshkey", "SSHKeys"),
    ];
    for (path, resource_name) in resource_pairs {
        let resources = api::request_search_api(path, resource_name, None, None, None, 50).await?;

        // filter shared resources
        let resources = resources.iter().filter(|v| { v["Scope"].as_str().map(|s| s == "user").unwrap_or(true) }).collect::<Vec<_>>();

        if 0 < resources.len() {
            println!("{}", resource_name);
            for resource in resources {
                match api::get_resource_id(&resource) {
                    Ok(resource_id) => {
                        if let Some(resource_name) = resource["Name"].as_str() {
                            println!("{}: {}", resource_id, resource_name);
                        } else {
                            println!("{}: {}", resource_id, to_string_pretty(&resource).expect("must be valid json"));
                        }
                    }
                    Err(api::Error::ResourceApiNoResourceId(_)) => {
                        if let Some(resource_name) = resource["Name"].as_str() {
                            println!("{}", resource_name);
                        } else {
                            println!("{}", to_string_pretty(&resource).expect("must be valid json"));
                        }
                    }
                    Err(e) => return Err(e),
                };
            }
            println!("----------");
        }
    }
    Ok(())
}

pub(crate) async fn show_env(prefix: impl AsRef<str>) -> Result<(), Error> {
    let prefix = prefix.as_ref();
    match search_ssh_public_key(prefix).await {
        Ok((key_id, key)) => {
            println!("Key {}: {}", key_id, to_string_pretty(&key).expect("must be valid json"));
            println!("----------");
        },
        Err(Error::ResourceNotFound(_)) => {
            println!("Key not found");
            println!("----------");
        },
        Err(e) => return Err(e),
    };

    match search_primary_server(prefix).await {
        Ok((server_id, server)) => {
            println!("Server {}: {}", server_id, to_string_pretty(&server).expect("must be valid json"));
            println!("----------");
        },
        Err(Error::ResourceNotFound(_)) => {
            println!("Server not found");
            println!("----------");
        },
        Err(e) => return Err(e),
    };

    match search_primary_server_disk(prefix).await {
        Ok((disk_id, disk)) => {
            println!("Disk {}: {}", disk_id, to_string_pretty(&disk).expect("must be valid json"));
            println!("----------");
        },
        Err(Error::ResourceNotFound(_)) => {
            println!("Disk not found");
            println!("----------");
        },
        Err(e) => return Err(e),
    };

    match search_vpc_router(prefix).await {
        Ok((vpc_router_id, vpc_router)) => {
            println!("VPC Router {}: {}", vpc_router_id, to_string_pretty(&vpc_router).expect("must be valid json"));
            println!("----------");
        },
        Err(Error::ResourceNotFound(_)) => {
            println!("VPC Router not found");
            println!("----------");
        },
        Err(e) => return Err(e),
    };

    match search_switch(prefix).await {
        Ok((switch_id, switch)) => {
            println!("Switch {}: {}", switch_id, to_string_pretty(&switch).expect("must be valid json"));
            println!("----------");
        },
        Err(Error::ResourceNotFound(_)) => {
            println!("Switch not found");
            println!("----------");
        },
        Err(e) => return Err(e),
    };

    Ok(())
}

pub(crate) async fn update_vpc_router_config(prefix: impl AsRef<str>) -> Result<(), Error> {
    let prefix = prefix.as_ref();
    let (vpc_router_id, _) = search_vpc_router(prefix).await?;
    update_vpc_router_config(&vpc_router_id).await?;
    wait_appliance_available(&vpc_router_id).await?;
    Ok(())
}

pub(crate) async fn create_env(prefix: impl AsRef<str>, password: impl AsRef<str>, public_key_path: impl AsRef<Path>) -> Result<(), Error> {
    let prefix = prefix.as_ref();
    let password = password.as_ref();
    let public_key_path = public_key_path.as_ref();
    let public_key = match fs::read_to_string(&public_key_path).await {
        Ok(public_key) => public_key,
        Err(e) => return Err(Error::CouldntReadPublicKey(e, public_key_path.to_path_buf())),
    };

    let (vpc_router_id, _) = create_vpc_router(prefix).await?;
    wait_appliance_available(&vpc_router_id).await?;

    let (switch_id, _) = create_switch(prefix).await?;
    connect_vpc_router_to_switch(&vpc_router_id, &switch_id).await?;
    update_vpc_router_config(&vpc_router_id).await?;

    up_appliance(&vpc_router_id).await?;
    wait_appliance_up(&vpc_router_id).await?;
    wait_appliance_available(&vpc_router_id).await?;

    let key_id = register_ssh_public_key(prefix, public_key).await?;
    let (archive_id, _) = search_latest_ubuntu_public_archive().await?;

    let (server_id, _) = create_primary_server(prefix, switch_id).await?;
    let (disk_id, _) = create_primary_server_disk(prefix, &server_id, &archive_id, password, &key_id).await?;
    wait_disk_available(&disk_id).await?;
    wait_server_available(&server_id).await?;

    up_server(&server_id).await?;

    wait_server_up(&server_id).await?;
    wait_server_available(&server_id).await?;
    Ok(())
}
*/
