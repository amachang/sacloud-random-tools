use std::{path::PathBuf, io, time::Duration};
use clap::{Parser, Subcommand};
use tokio::{fs, time::sleep};
use serde::Serialize;

use crate::{
    api::{
        self,
        Server, ServerId,
        Switch, SwitchId,
        SshPublicKeyId,
        Appliance, ApplianceId,
        Archive,
        Disk,
        InstanceStatus,
    },
    service_env::{
        PrimaryVpcRouter,
        PrimarySwitch,
        PrimaryServer,
        PrimaryServerDisk,
        PrimaryServerSshPublicKey,
        PrimaryServerSetupShellNote,
    },
};

#[derive(Debug, Serialize)]
pub(crate) enum Error {
    PrimaryServerNotConnectedToSwitch(ServerId, SwitchId),
    PrimaryServerNotFoundAndNeedsToBeCreatedButLoginMethodNotGiven,
    PrimarySwitchNotConnectedToVpcRouter(SwitchId, ApplianceId),
    PrimarySshPublicKeyAlreadyRegisteredButMismatch(SshPublicKeyId, String, String),
    PrimarySshPublicKeyNotGivenForNewServerDisk,
    PrimarySshPublicKeyGivenButCouldntRead(PathBuf, String),
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
    Clean(CleanCmd),
}

impl Cmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        match self {
            Cmd::Update(cmd) => cmd.run().await,
            Cmd::Clean(cmd) => cmd.run().await,
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

        let login_method_supplies = self.pubkey.is_some() || self.password.is_some();

        let ssh_public_key = if let Some(ssh_public_key_path) = &self.pubkey {
            match fs::read_to_string(ssh_public_key_path).await {
                Ok(ssh_public_key) => Some(ssh_public_key),
                Err(e) => return Err(Error::PrimarySshPublicKeyGivenButCouldntRead(ssh_public_key_path.clone(), e.to_string())),
            }
        } else {
            None
        };
        let password = self.password.as_deref();

        let server = PrimaryServer::try_get(prefix).await?;
        if server.is_none() {
            if !login_method_supplies {
                return Err(Error::PrimaryServerNotFoundAndNeedsToBeCreatedButLoginMethodNotGiven);
            }
            log::info!("[CHECKED] login method check for server creation: ok");
        }

        // VPC Router
        let vpc_router = if let Some(vpc_router) = PrimaryVpcRouter::try_get(prefix).await? {
            log::info!("[CHECKED] vpc router existence check: already exists, id: {}, ok", vpc_router.id());
            Appliance::wait_available(vpc_router.id()).await?;
            log::info!("[CHECKED] vpc router availability check: ok");
            vpc_router
        } else {
            log::info!("[START] vpc router existence check: not exists, creating...");
            let vpc_router = PrimaryVpcRouter::create(prefix).await?;
            log::info!("[DONE] vpc router created, id: {}, ok", vpc_router.id());

            log::info!("[START] vpc router wait available...");
            Appliance::wait_available(vpc_router.id()).await?;
            log::info!("[CHECKED] vpc router available, ok");
            vpc_router
        };

        // Switch
        let switch = if let Some(switch) = PrimarySwitch::try_get(prefix).await? {
            log::info!("[CHECKED] switch existence check: already exists, id: {}, ok", switch.id());
            let is_connected = Appliance::is_connected_to_switch(vpc_router.id(), switch.id()).await?;
            if !is_connected {
                return Err(Error::PrimarySwitchNotConnectedToVpcRouter(switch.id().clone(), vpc_router.id().clone()))
            }
            log::info!("[CHECKED] switch connection check: connected to vpc router, ok");
            switch
        } else {
            log::info!("[START] switch existence check: not exists, creating...");
            let switch = PrimarySwitch::create(prefix).await?;
            log::info!("[DONE] switch created, id: {}, ok", switch.id());
            log::info!("[START] switch connection check: connecting to vpc router...");
            Appliance::connect_to_switch(vpc_router.id(), switch.id()).await?;
            log::info!("[DONE] switch connected to vpc router, ok");
            switch
        };

        if Appliance::is_up(vpc_router.id()).await? {
            log::info!("[CHECKED] vpc router up check: ok");
            Appliance::wait_available(vpc_router.id()).await?;
            log::info!("[CHECKED] vpc router availability check: ok");
        } else {
            log::info!("[START] vpc router booting...");
            Appliance::up(vpc_router.id()).await?;
            Appliance::wait_up(vpc_router.id()).await?;
            log::info!("[DONE] vpc router booted, ok");

            log::info!("[START] vpc router wait available...");
            Appliance::wait_available(vpc_router.id()).await?;
            log::info!("[DONE] vpc router available, ok");
        }

        // Setup Script Note
        let note = if let Some(note) = PrimaryServerSetupShellNote::try_get(prefix).await? {
            log::info!("[CHECKED] note existence check: already exists, id: {}, ok", note.id());
            log::info!("[START] note content updating if needed...");
            PrimaryServerSetupShellNote::update_content_if_needed(note.id()).await?;
            log::info!("[DONE] note content updated, ok");
            note
        } else {
            log::info!("[START] note existence check: not exists, creating...");
            let note = PrimaryServerSetupShellNote::create(prefix).await?;
            log::info!("[DONE] note created, id: {}, ok", note.id());
            note
        };

        // Server
        let server = if let Some(server) = server {
            log::info!("[CHECKED] server existence check: already exists, id: {}, ok", server.id());
            let is_connected = Server::is_connected_to_switch(server.id(), switch.id()).await?;
            if !is_connected {
                return Err(Error::PrimaryServerNotConnectedToSwitch(server.id().clone(), switch.id().clone()))
            }
            log::info!("[CHECKED] server connection check: connected to switch, ok");
            server
        } else {
            log::info!("[START] server existence check: not exists, creating...");
            let server = PrimaryServer::create(prefix, switch.id()).await?;
            log::info!("[DONE] server created, id: {}, ok", server.id());
            server
        };

        // Disk
        if let Some(disk) = PrimaryServerDisk::try_get(prefix).await? {
            log::info!("[CHECKED] disk existence check: already exists, id: {}, ok", disk.id());
            Disk::wait_available(disk.id()).await?;
            log::info!("[CHECKED] disk availability check: ok");
        } else {
            let ssh_public_key = if let Some(current_ssh_public_key) = PrimaryServerSshPublicKey::try_get(prefix).await? {
                log::info!("[CHECKED] ssh public key existence check: already exists, id: {}, ok", current_ssh_public_key.id());
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
                log::info!("[CHECKED] ssh public key mismatch check: ok");
                current_ssh_public_key
            } else {
                log::info!("[CHECKED] ssh public key existence check: not exists");
                let Some(ssh_public_key) = ssh_public_key else {
                    return Err(Error::PrimarySshPublicKeyNotGivenForNewServerDisk);
                };
                log::info!("[START] ssh public key existence check: not exists, creating...");
                let ssh_public_key = PrimaryServerSshPublicKey::create(prefix, ssh_public_key).await?;
                log::info!("[DONE] ssh public key created, id: {}, ok", ssh_public_key.id());
                ssh_public_key
            };

            log::info!("[START] search latest public ubuntu archive...");
            let archive = Archive::latest_public_ubuntu().await?;
            log::info!("[DONE] search latest public ubuntu archive, id: {}, ok", archive.id());

            log::info!("[START] disk existence check: not exists, creating...");
            let disk = PrimaryServerDisk::create_for_server(prefix, server.id(), archive.id(), note.id(), ssh_public_key.id(), password).await?;
            log::info!("[DONE] disk created, id: {}, ok", disk.id());

            log::info!("[START] disk wait available...");
            Disk::wait_available(disk.id()).await?;
            log::info!("[DONE] disk available, ok");
        };

        Server::wait_available(server.id()).await?;
        log::info!("[CHECKED] server availability check: ok");

        if Server::is_up(server.id()).await? {
            log::info!("[CHECKED] server up check: ok");
        } else {
            log::info!("[START] server booting...");
            Server::up(server.id()).await?;
            Server::wait_up(server.id()).await?;
            log::info!("[DONE] server booted, ok");
        }
        Server::wait_available(server.id()).await?;
        log::info!("[CHECKED] server availability check: ok");

        log::info!("[START] vpc router config updating...");
        PrimaryVpcRouter::update_config(vpc_router.id()).await?;
        Appliance::apply_config(vpc_router.id()).await?;
        log::info!("[DONE] vpc router config updated, ok");

        Appliance::wait_available(vpc_router.id()).await?;
        log::info!("[CHECKED] vpc router availability check: ok");

        log::info!("[DONE] all checks passed, ok");
        Ok(())
    }
}

#[derive(Debug, Parser)]
pub(crate) struct CleanCmd {

    #[arg(long)]
    prefix: String,

    #[arg(long)]
    force: bool,
}

impl CleanCmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        let prefix = self.prefix.as_str();

        // confirm server down
        if !self.force {
            println!("Realy down? If ok, input the prefix again:");
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            if input.trim() != prefix {
                log::error!("prefix not matched");
                return Ok(());
            }
        }

        log::info!("[START] instance status check...");
        let vpc_router = PrimaryVpcRouter::try_get(prefix).await?;
        let switch = PrimarySwitch::try_get(prefix).await?;
        let server = PrimaryServer::try_get(prefix).await?;
        let disk = PrimaryServerDisk::try_get(prefix).await?;

        if let Some(vpc_router) = &vpc_router {
            loop {
                match Appliance::instance_status(vpc_router.id()).await {
                    Err(api::Error::ResourceUnknownInstanceStatus) => {
                        log::info!("[WAIT] vpc router instance status check: unknown, retrying...");
                        sleep(Duration::from_secs(5)).await;
                    },
                    Err(e) => return Err(e.into()),
                    Ok(InstanceStatus::Up | InstanceStatus::Down) => {
                        break;
                    },
                    Ok(status) => {
                        log::info!("[WAIT] vpc router instance status check: {}, retrying...", status);
                        sleep(Duration::from_secs(5)).await;
                    },
                }
            }
        }

        if let Some(server) = &server {
            loop {
                match Server::instance_status(server.id()).await {
                    Err(api::Error::ResourceUnknownInstanceStatus) => {
                        log::info!("[WAIT] server instance status check: unknown, retrying...");
                        sleep(Duration::from_secs(5)).await;
                    },
                    Err(e) => return Err(e.into()),
                    Ok(InstanceStatus::Up | InstanceStatus::Down) => {
                        break;
                    },
                    Ok(status) => {
                        log::info!("[WAIT] server instance status check: {}, retrying...", status);
                        sleep(Duration::from_secs(5)).await;
                    },
                }
            }
        }
        log::info!("[CHECKED] instance status check: ok");

        if let Some(vpc_router) = vpc_router {
            if Appliance::is_up(vpc_router.id()).await? {
                log::info!("[START] vpc router down...");
                Appliance::down(vpc_router.id()).await?;
                Appliance::wait_down(vpc_router.id()).await?;
                log::info!("[DONE] vpc router down: ok");
            }
            log::info!("[START] vpc router delete...");
            Appliance::delete(vpc_router.id()).await?;
            Appliance::wait_delete(vpc_router.id()).await?;
            log::info!("[DONE] vpc router delete: ok");
        }

        if let Some(server) = server {
            if Server::is_up(server.id()).await? {
                log::info!("[START] server down...");
                Server::down(server.id()).await?;
                Server::wait_down(server.id()).await?;
                log::info!("[DONE] server down: ok");
            }
            log::info!("[START] server delete...");
            Server::delete(server.id()).await?;
            Server::wait_delete(server.id()).await?;
            log::info!("[DONE] server delete: ok");
        }

        if let Some(disk) = disk {
            log::info!("[START] disk delete...");
            Disk::delete(disk.id()).await?;
            Disk::wait_delete(disk.id()).await?;
            log::info!("[DONE] disk delete: ok");
        }

        if let Some(switch) = switch {
            log::info!("[START] switch delete...");
            Switch::delete(switch.id()).await?;
            Switch::wait_delete(switch.id()).await?;
            log::info!("[DONE] switch delete: ok");
        }

        log::info!("[NOTE] ssh public key is not deleted for safety");
        log::info!("[DONE] all checks passed, ok");

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
*/

