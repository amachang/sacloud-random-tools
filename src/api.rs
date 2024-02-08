use std::{fmt, env, borrow::Borrow, time::Duration, collections::HashSet, net::Ipv4Addr};
use once_cell::sync::Lazy;
use url::Url;
use serde::{Serialize, Deserialize};
use serde_json::{self, Value, json};
use reqwest::{Method, StatusCode};
use tokio::time::sleep;

static ACCESS_TOKEN: Lazy<String> = Lazy::new(|| { env::var("SACLOUD_ACCESS_TOKEN").unwrap() });
static SECRET_TOKEN: Lazy<String> = Lazy::new(|| { env::var("SACLOUD_SECRET_TOKEN").unwrap() });
static API_BASE_URL: Lazy<Url> = Lazy::new(|| { Url::parse(format!("https://secure.sakura.ad.jp/cloud/zone/{}/api/cloud/1.1/", env::var("SACLOUD_ZONE").unwrap()).as_str()).unwrap() });

#[derive(Debug)]
pub(crate) enum Error {
    ResourceNotFound(String),
    TooManyResources(String, usize),
    ResourceSerializationFailed(ResourceKind, serde_json::Error),
    ResourceDeserializationFailed(ResourceKind, serde_json::Error),
    ResourceApiInvalidResourceObject(String, Option<Value>),
    ResourceApiInvalidStatusDataType(Value, Value, String, Option<Value>),
    ResourceApiInvalidStatusFalse(Value, String, Option<Value>),
    ResourceApiWaitStatusNotFound(String, Value),
    ResourceApiWaitStatusFailed(String, Value),
    ResourceApiWaitStatusUnknown(String, String, Value),
    RequestFailed(reqwest::Error, String, Option<Value>),
    InvalidResponseJson(reqwest::Error, String, Option<Value>),
    ApiBadRequest(String, Option<Value>),
    ApiUnauthorized(String, Option<Value>),
    ApiForbidden(String, Option<Value>),
    ApiNotFound(String, Option<Value>),
    ApiMethodNotAllowed(String, Option<Value>),
    ApiNotAcceptable(String, Option<Value>),
    ApiRequestTimeout(String, Option<Value>),
    ApiConflict(String, Option<Value>),
    ApiLengthRequired(String, Option<Value>),
    ApiPayloadTooLarge(String, Option<Value>),
    ApiUnsupportedMediaType(String, Option<Value>),
    ApiInternalServerError(String, Option<Value>),
    ApiServiceUnavailable(String, Option<Value>),
    ApiUnknownStatusCode(StatusCode, String, Option<Value>),
    SearchApiInvalidTotalCount(String, Value),
    SearchApiInvalidIndexFrom(Option<u64>, String, Value),
    SearchApiInvalidResourceCount(String, Value),
    SearchApiInvalidResourceArray(Value, String, Value),
    ResourceInfoLackOfRequiredField(String),
    ResourceInfoPasswordGivenButPwAuthDisabled,
    ResourceInfoPasswordNotGivenButPwAuthEnabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceKind {
    Server,
    Disk,
    SshPublicKey,
    Switch,
    Appliance,
    Archive,
    ServerPlan,
    DiskPlan,
    Note,
}

impl ResourceKind {
    pub(crate) fn single_name(&self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Disk => "Disk",
            Self::SshPublicKey => "SSHKey",
            Self::Switch => "Switch",
            Self::Appliance => "Appliance",
            Self::Archive => "Archive",
            Self::ServerPlan => "ServerPlan",
            Self::DiskPlan => "DiskPlan",
            Self::Note => "Note",
        }
    }

    pub(crate) fn prural_name(&self) -> &'static str {
        match self {
            Self::Server => "Servers",
            Self::Disk => "Disks",
            Self::SshPublicKey => "SSHKeys",
            Self::Switch => "Switches",
            Self::Appliance => "Appliances",
            Self::Archive => "Archives",
            Self::ServerPlan => "ServerPlans",
            Self::DiskPlan => "DiskPlans",
            Self::Note => "Notes",
        }
    }

    pub(crate) fn path(&self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Disk => "disk",
            Self::SshPublicKey => "sshkey",
            Self::Switch => "switch",
            Self::Appliance => "appliance",
            Self::Archive => "archive",
            Self::ServerPlan => "product/server",
            Self::DiskPlan => "product/disk",
            Self::Note => "note",
        }
    }

    pub(crate) async fn search_by_name(&self, name: impl AsRef<str>) -> Result<Option<Value>, Error> {
        let name = name.as_ref();
        let path = self.path();
        let resource_name = self.prural_name();
        let filter = json!({ "Name": [ name ] });
        search_single_resource(path, filter, resource_name).await
    }

    pub(crate) async fn search_one_by_tags(&self, tags: Vec<&str>) -> Result<Option<Value>, Error> {
        let path = self.path();
        let resource_name = self.prural_name();
        let tags = tags.iter().map(|s| Value::from(s.to_string())).collect::<Vec<_>>();
        let filter = json!({ "Tags": tags });
        search_single_resource(path, filter, resource_name).await
    }

    pub(crate) async fn create(&self, resource_value: Value) -> Result<Value, Error> {
        let path = self.path();
        let resource_name = self.single_name();
        create(path, json!({ resource_name: resource_value }), resource_name).await
    }

    pub(crate) async fn up_resource(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        update(format!("{}/{}/power", self.path(), resource_id), None).await
    }

    pub(crate) async fn wait_available(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        let path = format!("{}/{}", self.path(), resource_id);
        wait_resource_available(&path, self.single_name()).await
    }

    pub(crate) async fn wait_up(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        let path = format!("{}/{}", self.path(), resource_id);
        wait_resource_up(&path, self.single_name()).await
    }
}


// Archive

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArchiveId(String);

impl fmt::Display for ArchiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ArchiveId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ArchiveRef {
    #[serde(rename = "ID")]
    id: ArchiveId,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Archive {
    #[serde(rename = "ID")]
    id: ArchiveId,
}

impl Archive {
    pub(crate) async fn latest_public_ubuntu() -> Result<Archive, Error> {
        let resource_value = ResourceKind::Archive.search_one_by_tags(vec!["ubuntu-22.04-latest"]).await?;
        let Some(resource_value) = resource_value else {
            return Err(Error::ResourceNotFound("Archive".to_string()));
        };
        Archive::from_value(resource_value)
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Archive, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Archive
    }

    pub(crate) fn id(&self) -> &ArchiveId {
        &self.id
    }
}


// Server

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerId(String);

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ServerId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ServerRef {
    #[serde(rename = "ID")]
    id: ServerId,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Server {
    #[serde(rename = "ID")]
    id: ServerId,

    #[serde(flatten)]
    info: ServerInfo,
}

impl Server {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::Server.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn create(info: ServerInfo) -> Result<Server, Error> {
        let req_value = info.to_value()?;
        let res_value = ResourceKind::Server.create(req_value).await?;
        Server::from_value(res_value)
    }

    pub(crate) async fn is_connected_to_switch(server_id: impl Borrow<ServerId>, switch_id: impl Borrow<SwitchId>) -> Result<bool, Error> {
        let server_id = server_id.borrow();
        let switch_id = switch_id.borrow();
        let servers = Switch::connected_servers(switch_id).await?;
        Ok(servers.iter().any(|server| server.id() == server_id))
    }

    pub(crate) async fn wait_available(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.wait_available(server_id.to_string()).await
    }

    pub(crate) async fn up(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.up_resource(server_id.to_string()).await
    }

    pub(crate) async fn wait_up(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.wait_up(server_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Server, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Server
    }

    pub(crate) fn id(&self) -> &ServerId {
        &self.id
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ServerInfo {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "ServerPlan")]
    server_plan: ServerPlanRef,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "HostName", skip_serializing_if = "Option::is_none")]
    host_name: Option<String>,

    #[serde(rename = "InterfaceDriver", default)]
    interface_driver: InterfaceDriver,


    // XXX probably used follows only creation time

    #[serde(rename = "ConnectedSwitches", skip_serializing_if = "Option::is_none")]
    connected_switches: Option<Vec<ConnectedSwitch>>,

    #[serde(rename = "WaitDiskMigration", skip_serializing_if = "Option::is_none")]
    wait_disk_migration: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ConnectedSwitch {
    Shared,
    Switch(SwitchRef),
}

impl ServerInfo {
    pub(crate) fn builder() -> ServerInfoBuilder {
        ServerInfoBuilder::new()
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::ServerPlan, e))
    }
}

#[derive(Debug)]
pub(crate) struct ServerInfoBuilder {
    name: Option<String>,
    server_plan: Option<ServerPlanRef>,
    description: Option<String>,
    host_name: Option<String>,
    interface_driver: InterfaceDriver,
    connected_switches: Option<Vec<ConnectedSwitch>>,
    wait_disk_migration: Option<bool>,
}

impl ServerInfoBuilder {
    pub(crate) fn new() -> Self {
        Self {
            name: None,
            server_plan: None,
            description: None,
            host_name: None,
            interface_driver: InterfaceDriver::default(),
            connected_switches: None,
            wait_disk_migration: None,
        }
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(crate) fn server_plan(mut self, server_plan_id: ServerPlanId) -> Self {
        self.server_plan = Some(ServerPlanRef { id: server_plan_id });
        self
    }

    pub(crate) fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(crate) fn host_name(mut self, host_name: impl Into<String>) -> Self {
        self.host_name = Some(host_name.into());
        self
    }

    pub(crate) fn interface_driver(mut self, interface_driver: InterfaceDriver) -> Self {
        self.interface_driver = interface_driver;
        self
    }

    pub(crate) fn connected_switch_ids(mut self, connected_switches: Vec<SwitchId>) -> Self {
        self.connected_switches = Some(connected_switches.into_iter().map(|id| ConnectedSwitch::Switch(SwitchRef { id })).collect());
        self
    }

    pub(crate) fn wait_disk_migration(mut self, wait_disk_migration: bool) -> Self {
        self.wait_disk_migration = Some(wait_disk_migration);
        self
    }

    pub(crate) fn build(self) -> Result<ServerInfo, Error> {
        let Some(name) = self.name else {
            return Err(Error::ResourceInfoLackOfRequiredField("name".to_string()));
        };
        let Some(server_plan) = self.server_plan else {
            return Err(Error::ResourceInfoLackOfRequiredField("server_plan".to_string()));
        };
        Ok(ServerInfo {
            name: name,
            server_plan: server_plan,
            description: self.description,
            host_name: self.host_name,
            interface_driver: self.interface_driver,
            connected_switches: self.connected_switches,
            wait_disk_migration: self.wait_disk_migration,
        })
    }
}


// ServerPlan

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerPlanId(String);

impl fmt::Display for ServerPlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ServerPlanId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ServerPlanRef {
    #[serde(rename = "ID")]
    id: ServerPlanId,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ServerPlan {
    #[serde(rename = "ID")]
    id: ServerPlanId,
}

impl ServerPlan {
    pub(crate) fn from_value(value: Value) -> Result<ServerPlan, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::ServerPlan, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::ServerPlan
    }

    pub(crate) fn id(&self) -> &ServerPlanId {
        &self.id
    }
}



// Switch

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SwitchId(String);

impl fmt::Display for SwitchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SwitchId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SwitchRef {
    #[serde(rename = "ID")]
    id: SwitchId,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Switch {
    #[serde(rename = "ID")]
    id: SwitchId,
}

impl Switch {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::Switch.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn connected_servers(switch_id: impl Borrow<SwitchId>) -> Result<Vec<Server>, Error> {
        let switch_id = switch_id.borrow();
        let resource_values = search(format!("switch/{}/server", switch_id), "Servers", None, None, None, 50).await?;
        let mut servers = Vec::new();
        for resource_value in resource_values {
            let server = Server::from_value(resource_value)?;
            servers.push(server);
        }
        Ok(servers)
    }

    pub(crate) async fn connected_appliances(switch_id: impl Borrow<SwitchId>) -> Result<Vec<Appliance>, Error> {
        let switch_id = switch_id.borrow();
        let resource_values = search(format!("switch/{}/appliance", switch_id), "Appliances", None, None, None, 50).await?;
        let mut appliances = Vec::new();
        for resource_value in resource_values {
            let appliance = Appliance::from_value(resource_value)?;
            appliances.push(appliance);
        }
        Ok(appliances)
    }

    pub(crate) async fn wait_available(switch_id: impl Borrow<SwitchId>) -> Result<(), Error> {
        let switch_id = switch_id.borrow();
        ResourceKind::Switch.wait_available(switch_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Switch, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Switch
    }

    pub(crate) fn id(&self) -> &SwitchId {
        &self.id
    }
}


// Appliance

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ApplianceId(String);

impl fmt::Display for ApplianceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ApplianceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Appliance {
    #[serde(rename = "ID")]
    id: ApplianceId,
}

impl Appliance {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::Appliance.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn connect_to_switch(appliance_id: impl Borrow<ApplianceId>, switch_id: impl Borrow<SwitchId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        let switch_id = switch_id.borrow();
        update(format!("appliance/{}/interface/1/to/switch/{}", appliance_id, switch_id), None).await
    }

    pub(crate) async fn is_connected_to_switch(appliance_id: impl Borrow<ApplianceId>, switch_id: impl Borrow<SwitchId>) -> Result<bool, Error> {
        let appliance_id = appliance_id.borrow();
        let switch_id = switch_id.borrow();
        let appliances = Switch::connected_appliances(switch_id).await?;
        Ok(appliances.iter().any(|appliance| appliance.id() == appliance_id))
    }
    
    pub(crate) async fn apply_config(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        update(format!("appliance/{}/config", appliance_id), None).await
    }

    pub(crate) async fn wait_available(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.wait_available(appliance_id.to_string()).await
    }

    pub(crate) async fn up(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.up_resource(appliance_id.to_string()).await
    }

    pub(crate) async fn wait_up(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.wait_up(appliance_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Appliance, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Appliance
    }

    pub(crate) fn id(&self) -> &ApplianceId {
        &self.id
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ApplianceInfo {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(flatten)]
    class: ApplianceClass,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ApplianceClass {

    #[serde(rename = "vpcrouter")]
    VpcRouter(VpcRouterInfo),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct VpcRouterInfo {
    #[serde(rename = "Switch")]
    switch: SwitchRef,
}

// Disk

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskId(String);

impl fmt::Display for DiskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DiskId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Disk {
    #[serde(rename = "ID")]
    id: DiskId,

    #[serde(flatten)]
    info: DiskInfo,
}

impl Disk {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::Disk.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn create(info: DiskInfo, config: DiskConfig) -> Result<Disk, Error> {
        let info_value = info.to_value()?;
        let config_value = config.to_value()?;

        let disk_resource_name = ResourceKind::Disk.single_name();
        let res_value = create(ResourceKind::Disk.path(), json!({ disk_resource_name: info_value, "Config": config_value }), disk_resource_name).await?;
        Disk::from_value(res_value)
    }

    pub(crate) async fn wait_available(disk_id: impl Borrow<DiskId>) -> Result<(), Error> {
        let disk_id = disk_id.borrow();
        ResourceKind::Disk.wait_available(disk_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Disk, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Disk
    }

    pub(crate) fn id(&self) -> &DiskId {
        &self.id
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DiskInfo {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "Plan")]
    plan: DiskPlanRef,

    #[serde(rename = "SourceArchive")]
    source_archive: ArchiveRef,

    #[serde(rename = "SizeMB")]
    size_mb: u64,

    #[serde(rename = "Connection")]
    connection: DiskConnection,

    #[serde(rename = "Server")]
    server: ServerRef,
}

impl DiskInfo {
    pub(crate) fn builder() -> DiskInfoBuilder {
        DiskInfoBuilder::new() 
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Disk, e))
    }
}

#[derive(Debug)]
pub(crate) struct DiskInfoBuilder {
    name: Option<String>,
    description: Option<String>,
    plan: Option<DiskPlanRef>,
    source_archive: Option<ArchiveRef>,
    size_mb: Option<u64>,
    connection: DiskConnection,
    server: Option<ServerRef>,
}

impl DiskInfoBuilder {
    fn new() -> Self {
        Self {
            name: None,
            description: None,
            plan: None,
            source_archive: None,
            size_mb: None,
            connection: DiskConnection::Virtio,
            server: None,
        }
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(crate) fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(crate) fn plan_id(mut self, plan_id: DiskPlanId) -> Self {
        self.plan = Some(DiskPlanRef { id: plan_id });
        self
    }

    pub(crate) fn source_archive_id(mut self, archive_id: ArchiveId) -> Self {
        self.source_archive = Some(ArchiveRef { id: archive_id });
        self
    }

    pub(crate) fn size_mb(mut self, size_mb: u64) -> Self {
        self.size_mb = Some(size_mb);
        self
    }
    
    pub(crate) fn connection(mut self, connection: DiskConnection) -> Self {
        self.connection = connection;
        self
    }

    pub(crate) fn server_id(mut self, server_id: ServerId) -> Self {
        self.server = Some(ServerRef { id: server_id });
        self
    }

    pub(crate) fn build(self) -> Result<DiskInfo, Error> {
        let Some(name) = self.name else {
            return Err(Error::ResourceInfoLackOfRequiredField("name".to_string()));
        };
        let Some(plan) = self.plan else {
            return Err(Error::ResourceInfoLackOfRequiredField("plan".to_string()));
        };
        let Some(source_archive) = self.source_archive else {
            return Err(Error::ResourceInfoLackOfRequiredField("source_archive".to_string()));
        };
        let Some(size_mb) = self.size_mb else {
            return Err(Error::ResourceInfoLackOfRequiredField("size_mb".to_string()));
        };
        let Some(server) = self.server else {
            return Err(Error::ResourceInfoLackOfRequiredField("server".to_string()));
        };
        Ok(DiskInfo {
            name: name,
            description: self.description,
            plan: plan,
            source_archive: source_archive,
            size_mb: size_mb,
            connection: self.connection,
            server: server,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum DiskConnection {
    #[serde(rename = "virtio")]
    Virtio,

    #[serde(rename = "ide")]
    Ide,
}


#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DiskConfig {
    #[serde(rename = "Password", skip_serializing_if = "Option::is_none")]
    password: Option<String>,

    #[serde(rename = "HostName", skip_serializing_if = "Option::is_none")]
    host_name: Option<String>,

    #[serde(rename = "SSHKeys")]
    ssh_keys: Vec<SshPublicKeyRef>,

    #[serde(rename = "ChangePartitionUUID", default)]
    change_partition_uuid: bool,

    #[serde(rename = "DisablePWAuth", default)]
    disable_pw_auth: bool,

    #[serde(rename = "UserIPAddress")]
    user_ip_address: Ipv4Addr,

    #[serde(rename = "UserIpv4Net")]
    user_subnet: Ipv4Net,

    #[serde(rename = "EnableDHCP", default)]
    enable_dhcp: bool,

    #[serde(rename = "Notes", default)]
    notes: Vec<NoteRef>,
}

impl DiskConfig {
    pub(crate) fn builder() -> DiskConfigBuilder {
        DiskConfigBuilder::new() 
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Disk, e))
    }
}

#[derive(Debug)]
pub(crate) struct DiskConfigBuilder {
    password: Option<String>,
    host_name: Option<String>,
    ssh_keys: Vec<SshPublicKeyRef>,
    change_partition_uuid: bool,
    disable_pw_auth: bool,
    user_ip_address: Option<Ipv4Addr>,
    user_subnet: Option<Ipv4Net>,
    enable_dhcp: bool,
    notes: Vec<NoteRef>,
}

impl DiskConfigBuilder {
    fn new() -> Self {
        Self {
            password: None,
            host_name: None,
            ssh_keys: Vec::new(),
            change_partition_uuid: false,
            disable_pw_auth: false,
            user_ip_address: None,
            user_subnet: None,
            enable_dhcp: false,
            notes: Vec::new(),
        }
    }

    pub(crate) fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    pub(crate) fn host_name(mut self, host_name: impl Into<String>) -> Self {
        self.host_name = Some(host_name.into());
        self
    }

    pub(crate) fn ssh_key_ids(mut self, ssh_key_ids: Vec<SshPublicKeyId>) -> Self {
        self.ssh_keys = ssh_key_ids.into_iter().map(|id| SshPublicKeyRef { id }).collect();
        self
    }

    pub(crate) fn change_partition_uuid(mut self, change_partition_uuid: bool) -> Self {
        self.change_partition_uuid = change_partition_uuid;
        self
    }

    pub(crate) fn disable_pw_auth(mut self, disable_pw_auth: bool) -> Self {
        self.disable_pw_auth = disable_pw_auth;
        self
    }

    pub(crate) fn user_ip_address(mut self, user_ip_address: Ipv4Addr) -> Self {
        self.user_ip_address = Some(user_ip_address);
        self
    }

    pub(crate) fn user_subnet(mut self, user_subnet: Ipv4Net) -> Self {
        self.user_subnet = Some(user_subnet);
        self
    }

    pub(crate) fn enable_dhcp(mut self, enable_dhcp: bool) -> Self {
        self.enable_dhcp = enable_dhcp;
        self
    }
    
    pub(crate) fn note_id_and_variables_pairs(mut self, note_ids: Vec<(NoteId, Value)>) -> Self {
        self.notes = note_ids.into_iter().map(|(id, variables)| NoteRef::new(id, variables)).collect();
        self
    }

    pub(crate) fn build(self) -> Result<DiskConfig, Error> {
        if self.ssh_keys.is_empty() {
            return Err(Error::ResourceInfoLackOfRequiredField("ssh_keys".to_string()));
        }
        let Some(user_ip_address) = self.user_ip_address else {
            return Err(Error::ResourceInfoLackOfRequiredField("user_ip_address".to_string()));
        };
        let Some(user_subnet) = self.user_subnet else {
            return Err(Error::ResourceInfoLackOfRequiredField("user_subnet".to_string()));
        };
        if self.password.is_some() && self.disable_pw_auth {
            return Err(Error::ResourceInfoPasswordGivenButPwAuthDisabled);
        }
        if self.password.is_none() && !self.disable_pw_auth {
            return Err(Error::ResourceInfoPasswordNotGivenButPwAuthEnabled);
        }

        Ok(DiskConfig {
            password: self.password,
            host_name: self.host_name,
            ssh_keys: self.ssh_keys,
            change_partition_uuid: self.change_partition_uuid,
            disable_pw_auth: self.disable_pw_auth,
            user_ip_address: user_ip_address,
            user_subnet: user_subnet,
            enable_dhcp: self.enable_dhcp,
            notes: self.notes,
        })
    }
}

// DiskPlan

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskPlanId(u64);

impl fmt::Display for DiskPlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DiskPlanId {
    fn from(s: String) -> Self {
        Self(s.parse().unwrap())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DiskPlanRef {
    #[serde(rename = "ID")]
    id: DiskPlanId,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DiskPlan {
    #[serde(rename = "ID")]
    id: DiskPlanId,
}

impl DiskPlan {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::DiskPlan.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn wait_available(disk_plan_id: impl Borrow<DiskPlanId>) -> Result<(), Error> {
        let disk_plan_id = disk_plan_id.borrow();
        ResourceKind::DiskPlan.wait_available(disk_plan_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::DiskPlan, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::DiskPlan
    }

    pub(crate) fn id(&self) -> &DiskPlanId {
        &self.id
    }
}


// SshPublicKey

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SshPublicKeyId(String);

impl fmt::Display for SshPublicKeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SshPublicKeyId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SshPublicKeyRef {
    #[serde(rename = "ID")]
    id: SshPublicKeyId,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SshPublicKey {
    #[serde(rename = "ID")]
    id: SshPublicKeyId,

    #[serde(flatten)]
    info: SshPublicKeyInfo,
}

impl SshPublicKey {
    pub(crate) fn public_key(&self) -> &str {
        &self.info.public_key   
    }

    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::SshPublicKey.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn create(info: SshPublicKeyInfo) -> Result<SshPublicKey, Error> {
        let req_value = info.to_value()?;
        let res_value = ResourceKind::SshPublicKey.create(req_value).await?;
        SshPublicKey::from_value(res_value)
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::SshPublicKey, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::SshPublicKey
    }

    pub(crate) fn id(&self) -> &SshPublicKeyId {
        &self.id
    }
}


#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SshPublicKeyInfo {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "PublicKey")]
    public_key: String,
}

impl SshPublicKeyInfo {
    pub(crate) fn builder() -> SshPublicKeyInfoBuilder {
        SshPublicKeyInfoBuilder::new()
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::SshPublicKey, e))
    }
}

#[derive(Debug)]
pub(crate) struct SshPublicKeyInfoBuilder {
    name: Option<String>,
    description: Option<String>,
    public_key: Option<String>,
}

impl SshPublicKeyInfoBuilder {
    fn new() -> Self {
        Self {
            name: None,
            description: None,
            public_key: None,
        }
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(crate) fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(crate) fn public_key(mut self, public_key: impl Into<String>) -> Self {
        self.public_key = Some(public_key.into());
        self
    }

    pub(crate) fn build(self) -> Result<SshPublicKeyInfo, Error> {
        let Some(name) = self.name else {
            return Err(Error::ResourceInfoLackOfRequiredField("name".to_string()));
        };
        let Some(public_key) = self.public_key else {
            return Err(Error::ResourceInfoLackOfRequiredField("public_key".to_string()));
        };
        Ok(SshPublicKeyInfo {
            name: name,
            description: self.description,
            public_key: public_key,
        })
    }
}

// Note

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NoteId(String);

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for NoteId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct NoteRef {
    #[serde(rename = "ID")]
    id: NoteId,

    #[serde(rename = "Variables")]
    variables: Value,
}

impl NoteRef {
    pub(crate) fn new(id: NoteId, variables: Value) -> Self {
        Self {
            id: id,
            variables: variables,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Note {
    #[serde(rename = "ID")]
    id: NoteId,
}

impl Note {
    pub(crate) async fn official_startup_script() -> Result<Note, Error> {
        let resource_value = ResourceKind::Note.search_by_name("sys-startup-preinstall").await?;
        let Some(resource_value) = resource_value else {
            return Err(Error::ResourceNotFound("Note".to_string()));
        };
        Note::from_value(resource_value)
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Note, e))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Note
    }

    pub(crate) fn id(&self) -> &NoteId {
        &self.id
    }
}


#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IpAddressRef {
    #[serde(rename = "IPAddress")]
    ip_address: Ipv4Addr,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Ipv4Net {
    #[serde(rename = "DefaultRoute")]
    default_route: Ipv4Addr,

    #[serde(rename = "NetworkMaskLen")]
    network_mask_len: u8,
}

impl Ipv4Net {
    pub(crate) fn new(default_route: Ipv4Addr, network_mask_len: u8) -> Self {
        Self {
            default_route: default_route,
            network_mask_len: network_mask_len,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SingleLineIpv4Net(String);

impl SingleLineIpv4Net {
    pub(crate) fn new(ip_address: Ipv4Addr, network_mask_len: u8) -> Self {
        let subnet = format!("{}/{}", ip_address, network_mask_len);
        Self(subnet)
    }
}

// InterfaceDriver
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum InterfaceDriver {
    #[serde(rename = "virtio")]
    Virtio,

    #[serde(rename = "e1000")]
    E1000,
}

impl Default for InterfaceDriver {
    fn default() -> Self {
        Self::Virtio
    }
}

// Utils

async fn search_single_resource(path: impl AsRef<str>, filter: Value, resource_name: impl AsRef<str>) -> Result<Option<Value>, Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    let mut resource_values = search(path, resource_name, Some(filter), None, None, 50).await?;

    if resource_values.len() > 1 {
        Err(Error::TooManyResources(resource_name.to_string(), resource_values.len()))
    }
    else if resource_values.len() < 1 {
        Ok(None)
    } else {
        let resource_value = resource_values[0].take();
        Ok(Some(resource_value))
    }
}

async fn wait_resource_up(path: impl AsRef<str>, resource_name: impl AsRef<str>) -> Result<(), Error> {
    wait_resource_status(path, resource_name,
        |res| res["Instance"]["Status"].as_str().map(|s| s.to_string()),
        ["cleaning"].into_iter().collect(),
        ["up"].into_iter().collect(),
        ["down"].into_iter().collect()).await
}

async fn wait_resource_available(path: impl AsRef<str>, resource_name: impl AsRef<str>) -> Result<(), Error> {
    wait_resource_status(path, resource_name,
        |res| res["Availability"].as_str().map(|s| s.to_string()),
        ["uploading", "migrating"].into_iter().collect(),
        ["available"].into_iter().collect(),
        ["failed"].into_iter().collect()).await
}

async fn wait_resource_status(path: impl AsRef<str>, resource_name: impl AsRef<str>, status_accessor_fn: impl Fn(&Value) -> Option<String>, working_value_set: HashSet<&str>, success_value_set: HashSet<&str>, failed_value_set: HashSet<&str>) -> Result<(), Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    loop {
        let resource = fetch(path, resource_name).await?;
        let Some(status) = status_accessor_fn(&resource) else {
            return Err(Error::ResourceApiWaitStatusNotFound(path.to_string(), resource.clone()));
        };
        let status: &str = &status;
        if failed_value_set.contains(status) {
            return Err(Error::ResourceApiWaitStatusFailed(path.to_string(), resource.clone()));
        }
        if success_value_set.contains(status) {
            break;
        }
        if !working_value_set.contains(status) {
            return Err(Error::ResourceApiWaitStatusUnknown(status.to_string(), path.to_string(), resource.clone()));
        }
        sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}


async fn create(path: impl AsRef<str>, body: Value, resource_name: impl AsRef<str>) -> Result<Value, Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    let resource = request_api_for_resource(Method::POST, path, Some(resource_name), Some(body.clone())).await?;
    Ok(resource)
}

async fn fetch(path: impl AsRef<str>, resource_name: impl AsRef<str>) -> Result<Value, Error> {
    let resource_name = resource_name.as_ref();
    request_api_for_resource(Method::GET, path, Some(resource_name), None).await
}

async fn update(path: impl AsRef<str>, body: Option<Value>) -> Result<(), Error> {
    let _ = request_api_for_resource(Method::PUT, path, None, body).await?;
    Ok(())
}

async fn delete(path: impl AsRef<str>, body: Value) -> Result<(), Error> {
    let _ = request_api_for_resource(Method::DELETE, path, None, Some(body)).await?;
    Ok(())
}

async fn search(path: impl AsRef<str>, resource_name: impl AsRef<str>, filter: Option<Value>, sort: Option<Value>, other: Option<Value>, page_count: u64) -> Result<Vec<Value>, Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    let mut result_resources = Vec::new();
    let mut index_from = 0;
    let query = if let Some(other) = other {
        other
    } else {
        json!({})
    };
    loop {
        let mut query = query.clone();
        query["From"] = Value::from(index_from);
        query["Count"] = Value::from(page_count);
        if let Some(filter) = filter.clone() {
            query["Filter"] = filter;
        }
        if let Some(sort) = sort.clone() {
            query["Sort"] = sort;
        }

        let query = Some(query);
        let value = request_api(Method::GET, path, &query, &None).await?;

        let query = query.expect("must be Some");
        let Some(total) = value["Total"].as_u64() else {
            return Err(Error::SearchApiInvalidTotalCount(path.to_string(), query.clone()));
        };
        let Some(response_index_from) = value["From"].as_u64() else {
            return Err(Error::SearchApiInvalidIndexFrom(None, path.to_string(), query.clone()));
        };

        if index_from != response_index_from {
            return Err(Error::SearchApiInvalidIndexFrom(Some(response_index_from), path.to_string(), query.clone()));
        }

        let Some(count) = value["Count"].as_u64() else {
            return Err(Error::SearchApiInvalidResourceCount(path.to_string(), query.clone()));
        };

        let Some(resources) = value[resource_name].as_array() else {
            return Err(Error::SearchApiInvalidResourceArray(value, path.to_string(), query.clone()));
        };
        result_resources.extend(resources.to_vec());

        if index_from + count >= total {
            break;
        }

        index_from += count;
    };
    Ok(result_resources)
}

async fn request_api_for_resource(method: Method, path: impl AsRef<str>, resource_name: Option<&str>, body: Option<Value>) -> Result<Value, Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    let mut value = request_api(method, path, &None, &body).await?;

    if let Some(is_ok) = value.get("is_ok") {
        let Some(is_ok) = is_ok.as_bool() else {
            return Err(Error::ResourceApiInvalidStatusDataType(is_ok.clone(), value.clone(), path.to_string(), body.clone()));
        };
        if !is_ok {
            return Err(Error::ResourceApiInvalidStatusFalse(value.clone(), path.to_string(), body.clone()));
        }
    }
    if let Some(success_status) = value.get("Success") {
        if let Some(success_status) = success_status.as_str() {
            if success_status != "Accepted" {
                return Err(Error::ResourceApiInvalidStatusFalse(value.clone(), path.to_string(), body.clone()));
            }
        } else if let Some(success_status) = success_status.as_bool() {
            if !success_status {
                return Err(Error::ResourceApiInvalidStatusFalse(value.clone(), path.to_string(), body.clone()));
            }
        } else {
            return Err(Error::ResourceApiInvalidStatusDataType(success_status.clone(), value.clone(), path.to_string(), body.clone()));
        }
    }

    if let Some(resource_name) = resource_name {
        let resource = value[resource_name].take();
        if !resource.is_object() {
            return Err(Error::ResourceApiInvalidResourceObject(path.to_string(), body.clone()));
        };
        Ok(resource)
    } else {
        Ok(value)
    }
}

async fn request_api(method: Method, path: impl AsRef<str>, query: &Option<Value>, body: &Option<Value>) -> Result<Value, Error> {
    let path = path.as_ref();
    log::trace!("START API REQUEST: method={:?}, path={}, query={}, body={}", method, path, query.clone().unwrap_or_default(), body.clone().unwrap_or_default());

    let mut url = API_BASE_URL.join(path).expect("must be valid url");
    if let Some(query) = query {
        url.set_query(Some(&query.to_string()));
    }
    let client = reqwest::Client::new();
    let mut req = client.request(method, url)
        .basic_auth(&*ACCESS_TOKEN, Some(&*SECRET_TOKEN));
    if let Some(body) = body {
        req = req.json(&body)
    };

    let res = req.send().await;

    let res = match res {
        Ok(res) => res,
        Err(e) => {
            log::trace!("ERROR API REQUEST: error={:?}", e);
            return Err(Error::RequestFailed(e, path.to_string(), body.clone()));
        },
    };

    // comments imported from https://developer.sakura.ad.jp/cloud/api/1.1/
    match res.status() {
        StatusCode::OK => {
            // 200 OK	正常に処理され、何らかのレスポンスが返却された。
            ()
        },
        StatusCode::CREATED => {
            // 201 Created	正常に処理され、何らかのリソースが作成された。 例：仮想サーバを作成した
            ()
        },
        StatusCode::ACCEPTED => {
            // 202 Accepted	正常に受け付けられたが、処理は完了していない。 例：ディスクの複製を開始したが、まだ完了していない
            ()
        },
        StatusCode::NO_CONTENT => {
            // 204 No Content	正常に処理され、空のレスポンスが返却された。
            ()
        },
        status_code => {
            log::trace!("ERROR API REQUEST: response={:?}", res);
            match status_code {
                StatusCode::BAD_REQUEST => {
                    // 400 Bad Request	リクエストパラメータが不正等。 例：許可されないフィールドに対し、負の値、過去の日付、異なる型の値等が指定されている
                    return Err(Error::ApiBadRequest(path.to_string(), body.clone()));
                },
                StatusCode::UNAUTHORIZED => {
                    // 401 Unauthorized	認証に失敗した。
                    return Err(Error::ApiUnauthorized(path.to_string(), body.clone()));
                },
                StatusCode::FORBIDDEN => {
                    // 403 Forbidden	リソースへのアクセス権限がない。 例：/user/sakurai というリソースの上位にある /user にアクセスしたが、このリソースは一般ユーザにはアクセスできない。
                    return Err(Error::ApiForbidden(path.to_string(), body.clone()));
                },
                StatusCode::NOT_FOUND => {
                    // 404 Not Found	リソースが存在しない。 例：taroというユーザはいないのに /user/taro というリソースにアクセスした。
                    return Err(Error::ApiNotFound(path.to_string(), body.clone()));
                },
                StatusCode::METHOD_NOT_ALLOWED => {
                    // 405 Method Not Allowed	要求されたメソッドは非対応。 例：/zone/5 というリソースにPUTメソッドは許可されていない。
                    return Err(Error::ApiMethodNotAllowed(path.to_string(), body.clone()));
                },
                StatusCode::NOT_ACCEPTABLE => {
                    // 406 Not Acceptable	何らかの事情でリクエストを受け入れられない。 例：残りの空きリソースがない
                    return Err(Error::ApiNotAcceptable(path.to_string(), body.clone()));
                },
                StatusCode::REQUEST_TIMEOUT => {
                    // 408 Request Time-out	リクエストがタイムアウトした。
                    return Err(Error::ApiRequestTimeout(path.to_string(), body.clone()));
                },
                StatusCode::CONFLICT => {
                    // 409 Conflict	リソースの現在の状態と矛盾する操作を行おうとした。 例：仮想サーバの電源が既に入っているのに、電源を投入しようとした。
                    return Err(Error::ApiConflict(path.to_string(), body.clone()));
                },
                StatusCode::LENGTH_REQUIRED => {
                    // 411 Length Required	リクエストヘッダーにLengthが含まれていない。curlコマンドの場合、curl -d ''で回避できる。
                    return Err(Error::ApiLengthRequired(path.to_string(), body.clone()));
                },
                StatusCode::PAYLOAD_TOO_LARGE => {
                    // 413 Request Entity Too Large	リクエストされた処理にかかる負荷が対応可能な範囲を越えた。 例：アップロードファイルのサイズ制限を越えた
                    return Err(Error::ApiPayloadTooLarge(path.to_string(), body.clone()));
                },
                StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                    // 415 Unsupported Media Type	リクエストされたフォーマットに対応していない。 例：画像データを返すリソースに対し、CSVフォーマットを要求した。
                    return Err(Error::ApiUnsupportedMediaType(path.to_string(), body.clone()));
                },
                StatusCode::INTERNAL_SERVER_ERROR => {
                    // 500 Internal Server Error	内部エラーが発生した。 例：PHPエラーが発生した。
                    return Err(Error::ApiInternalServerError(path.to_string(), body.clone()));
                },
                StatusCode::SERVICE_UNAVAILABLE => {
                    // 503 Service Unavailable	何らかの事情によりサービスが利用可能でない。 例：DB接続に失敗した
                    return Err(Error::ApiServiceUnavailable(path.to_string(), body.clone()));
                },
                _ => {
                    return Err(Error::ApiUnknownStatusCode(res.status(), path.to_string(), body.clone()));
                },
            }
        },
    }

    let value = res.json().await.map_err(|e| Error::InvalidResponseJson(e, path.to_string(), body.clone()))?;
    log::trace!("END API REQUEST: value={:?}", value);
    Ok(value)
}

// test
#[cfg(test)]
mod tests {
    #[test]
    fn server_serializable() {

    }
}

