use std::{path::PathBuf, io, time::Duration, thread};
use clap::{Parser, Subcommand};
use tokio::{fs, time::sleep, runtime::Runtime, signal};
use serde::Serialize;
use dirs::home_dir;

use crate::{
    api::{
        self,
        Server, ServerId,
        Switch, SwitchId,
        SshPublicKeyId,
        Appliance, ApplianceId,
        Archive,
        Disk,
        Note,
        InstanceStatus,
    },
    service_env::{
        self,
        CONFIG,
        PRIMARY_SERVER_FORWARDED_PORT,
        PrimaryVpcRouter,
        PrimarySwitch,
        PrimaryServer,
        PrimaryServerDisk,
        PrimaryServerSshPublicKey,
        PrimaryServerSetupShellNote,
    },
    service_script::{
        self,
        ServiceScript,
    },
    ssh::{
        self,
        Session,
    },
};

#[derive(Debug, Serialize)]
pub(crate) enum Error {
    PrimaryServerNotConnectedToSwitch(ServerId, SwitchId),
    PrimarySwitchNotConnectedToVpcRouter(SwitchId, ApplianceId),
    PrimarySshPublicKeyAlreadyRegisteredButMismatch(SshPublicKeyId, String, String),
    PrimarySshPublicKeyNotGivenForNewServerDisk,
    PrimarySshPublicKeyGivenButCouldntRead(PathBuf, String),
    PrimaryVpcRouterNotExists,
    ServiceScriptError(service_script::Error),
    ApiError(api::Error),
    ServiceEnvError(service_env::Error),
    SshError(ssh::Error),
}

impl From<api::Error> for Error {
    fn from(e: api::Error) -> Self {
        Error::ApiError(e)
    }
}

impl From<service_env::Error> for Error {
    fn from(e: service_env::Error) -> Self {
        Error::ServiceEnvError(e)
    }
}

impl From<service_script::Error> for Error {
    fn from(e: service_script::Error) -> Self {
        Error::ServiceScriptError(e)
    }
}

impl From<ssh::Error> for Error {
    fn from(e: ssh::Error) -> Self {
        Error::SshError(e)
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum Cmd {
    SyncRemoteDir(SyncRemoteDirCmd),
    PortForwarding(PortForwardingCmd),
    Update(UpdateCmd),
    Clean(CleanCmd),
}

impl Cmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        match self {
            Cmd::SyncRemoteDir(cmd) => cmd.run().await,
            Cmd::PortForwarding(cmd) => cmd.run().await,
            Cmd::Update(cmd) => cmd.run().await,
            Cmd::Clean(cmd) => cmd.run().await,
        }
    }
}

#[derive(Debug, Parser)]
pub(crate) struct SyncRemoteDirCmd {
    #[arg(long, env = "SACLOUD_SERVICE_PREFIX")]
    prefix: String,

    #[arg(long)]
    privkey: Option<PathBuf>,

    #[arg(long)]
    local_dir: PathBuf,

    #[arg(long)]
    remote_dir: PathBuf,
}

impl SyncRemoteDirCmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        let prefix = self.prefix.as_str();
        let local_dir = self.local_dir.as_path();
        let remote_dir = self.remote_dir.as_path();
        let ssh_public_key_path = self.privkey.clone().unwrap_or(default_privkey_path());

        let Some(vpc_router) = PrimaryVpcRouter::try_get(prefix).await? else {
            return Err(Error::PrimaryVpcRouterNotExists);
        };
        let public_shared_ip = vpc_router.public_shared_ip()?;
        let session = Session::connect(public_shared_ip, PRIMARY_SERVER_FORWARDED_PORT, "ubuntu", ssh_public_key_path).await?;

        let result = session.sync_remote_dir(remote_dir, local_dir).await;
        let _ = session.close().await;
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug, Parser)]
pub(crate) struct PortForwardingCmd {
    #[arg(long, env = "SACLOUD_SERVICE_PREFIX")]
    prefix: String,

    #[arg(long)]
    privkey: Option<PathBuf>,
}

impl PortForwardingCmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        let prefix = self.prefix.as_str();
        let ssh_public_key_path = self.privkey.clone().unwrap_or(default_privkey_path());

        let Some(vpc_router) = PrimaryVpcRouter::try_get(prefix).await? else {
            return Err(Error::PrimaryVpcRouterNotExists);
        };
        let public_shared_ip = vpc_router.public_shared_ip()?;
        let session = Session::connect(public_shared_ip, PRIMARY_SERVER_FORWARDED_PORT, "ubuntu", ssh_public_key_path).await?;

        for forwarding_port in &CONFIG.forwarding_ports {
            log::info!("[START] port forwarding: {} -> {}", forwarding_port.remote_port, forwarding_port.local_port);
            match session.forward_remote_port(forwarding_port.remote_port, forwarding_port.local_port).await {
                Ok(_) => (),
                Err(e) => {
                    let _ = session.close().await;
                    return Err(Error::SshError(e));
                },
            }
            log::info!("[DONE] port forwarding: ok");
        }

        loop {
            // I don't know the proer way to keep the session alive
            tokio::select! {
                _ = signal::ctrl_c() => {
                    break;
                },
                _ = sleep(Duration::from_secs(5)) => {},
            }
        }
        let _ = session.close().await;
        Ok(())
    }
}

#[derive(Debug, Parser)]
pub(crate) struct UpdateCmd {
    #[arg(long, env = "SACLOUD_SERVICE_PREFIX")]
    prefix: String,

    #[arg(long)]
    pubkey: Option<PathBuf>,

    #[arg(long)]
    privkey: Option<PathBuf>,
}

impl UpdateCmd {
    pub(crate) async fn run(&self) -> Result<(), Error> {
        let prefix = self.prefix.as_str();
        let ssh_public_key_path = self.pubkey.clone().unwrap_or(default_pubkey_path());
        let ssh_private_key_path = self.privkey.clone().unwrap_or(default_privkey_path());

        let ssh_public_key = match fs::read_to_string(&ssh_public_key_path).await {
            Ok(ssh_public_key) => Some(ssh_public_key),
            Err(e) => return Err(Error::PrimarySshPublicKeyGivenButCouldntRead(ssh_public_key_path, e.to_string())),
        };

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

        // セットアップスクリプトのために一旦 Firewall は外す
        log::info!("[START] vpc router config update without firewall for setup script...");
        PrimaryVpcRouter::update_config(vpc_router.id(), false).await?;
        Appliance::apply_config(vpc_router.id()).await?;
        log::info!("[DONE] vpc router config updated without firewall, ok");

        // Guard で戻す
        struct FirewallGuard(ApplianceId);
        impl Drop for FirewallGuard {
            fn drop(&mut self) {
                log::info!("[IMPORTANT] ensure vpc router config with firewall...");
                let vpc_router_id = self.0.clone();
                let handler = thread::spawn(move || {
                    Runtime::new().expect("[FATAL_ERROR] failed to new runtime").block_on(async move {
                        PrimaryVpcRouter::update_config(&vpc_router_id, true).await
                            .expect("[FATAL_ERROR] failed to update vpc router config with firewall");
                        Appliance::apply_config(&vpc_router_id).await
                            .expect("[FATAL_ERROR] failed to apply vpc router config with firewall");
                        Appliance::wait_available(&vpc_router_id).await
                            .expect("[FATAL_ERROR] failed to wait vpc router available");
                        log::info!("[IMPORTANT] firewall ensured");
                    })
                });
                handler.join().expect("[FATAL_ERROR] failed to join handler");
            }
        }
        let _firewall_guard = FirewallGuard(vpc_router.id().clone());

        Appliance::wait_available(vpc_router.id()).await?;
        log::info!("[CHECKED] vpc router availability check: ok");

        // Server
        let server = if let Some(server) = PrimaryServer::try_get(prefix).await? {
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
            // Setup Startup Script
            let note = if let Some(note) = PrimaryServerSetupShellNote::try_get(prefix).await? {
                log::info!("[CHECKED] note existence check: already exists, id: {}, ok", note.id());
                log::info!("[START] note content updating if needed...");
                PrimaryServerSetupShellNote::update_content_if_needed(note.id()).await?;
                Note::wait_available(note.id()).await?;
                log::info!("[DONE] note content updated, ok");
                note
            } else {
                log::info!("[START] note existence check: not exists, creating...");
                let note = PrimaryServerSetupShellNote::create(prefix).await?;
                Note::wait_available(note.id()).await?;
                log::info!("[DONE] note created, id: {}, ok", note.id());
                note
            };

            // Setup SSH Public Key
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
            let disk = PrimaryServerDisk::create_for_server(prefix, server.id(), archive.id(), note.id(), ssh_public_key.id()).await?;
            log::info!("[DONE] disk created, id: {}, ok", disk.id());

            log::info!("[START] disk wait available...");
            Disk::wait_available(disk.id()).await?;
            log::info!("[DONE] disk available, ok");
        };

        Server::wait_available(server.id()).await?;
        log::info!("[CHECKED] server availability check: ok");

        if !Server::is_up(server.id()).await? {
            log::info!("[START] server booting...");
            Server::up(server.id()).await?;
            Server::wait_up(server.id()).await?;
            log::info!("[DONE] server booted, ok");
        }

        log::info!("[START] prepare setup script for server...");
        let Some(vpc_router) = PrimaryVpcRouter::try_get(prefix).await? else {
            return Err(Error::PrimaryVpcRouterNotExists);
        };
        let public_shared_ip = vpc_router.public_shared_ip()?;
        ServiceScript::prepare_for_server(public_shared_ip, "ubuntu", &ssh_private_key_path).await?;
        log::info!("[DONE] setup script prepared, ok");

        log::info!("[START] restart server for running setup script...");
        Server::down(server.id()).await?;
        Server::wait_down(server.id()).await?;
        Server::up(server.id()).await?;
        Server::wait_up(server.id()).await?;
        log::info!("[DONE] server restarted for running setup script, ok");

        log::info!("[START] wait for server setup script finished...");
        ServiceScript::wait_for_done(public_shared_ip, "ubuntu", &ssh_private_key_path).await?;
        log::info!("[DONE] server setup script finished, ok");

        Ok(())
    }
}

#[derive(Debug, Parser)]
pub(crate) struct CleanCmd {
    #[arg(long, env = "SACLOUD_SERVICE_PREFIX")]
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

fn default_pubkey_path() -> PathBuf {
    home_dir().expect("home dir is prerequisite").join(".ssh/id_rsa.pub")
}

fn default_privkey_path() -> PathBuf {
    home_dir().expect("home dir is prerequisite").join(".ssh/id_rsa")
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

