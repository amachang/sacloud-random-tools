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

#[derive(Debug, Serialize)]
pub(crate) enum Error {
    ResourceNotFound(String),
    TooManyResources(String, usize),
    ResourceUnknownInstanceStatus,
    ResourceSerializationFailed(ResourceKind, String),
    ResourceDeserializationFailed(ResourceKind, String),
    ResourceApiInvalidResourceObject(String, Option<Value>),
    ResourceApiInvalidStatusDataType(Value, Value, String, Option<Value>),
    ResourceApiInvalidStatusFalse(Value, String, Option<Value>),
    ResourceApiWaitStatusNotFound(String, Value),
    ResourceApiWaitStatusFailed(String, Value),
    ResourceApiWaitStatusUnknown(String, String, Value),
    RequestFailed(String, String, Option<Value>),
    InvalidResponseJson(String, String, Option<Value>),
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
    ApiUnknownStatusCode(u16, String, Option<Value>),
    SearchApiInvalidTotalCount(String, Value),
    SearchApiInvalidIndexFrom(Option<u64>, String, Value),
    SearchApiInvalidResourceCount(String, Value),
    SearchApiInvalidResourceArray(Value, String, Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum ResourceKind {
    Server,
    Disk,
    SshPublicKey,
    Switch,
    Appliance,
    Archive,
    ServerPlan,
    // DiskPlan, commented out because it's not used
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
            // Self::DiskPlan => "DiskPlan",
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
            // Self::DiskPlan => "DiskPlans",
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
            // Self::DiskPlan => "product/disk",
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

    pub(crate) async fn update(&self, resource_id: impl AsRef<str>, resource_value: Value) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        let path = format!("{}/{}", self.path(), resource_id);
        let resource_name = self.single_name();
        update(path, Some(json!({ resource_name: resource_value }))).await
    }

    pub(crate) async fn delete(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        let path = format!("{}/{}", self.path(), resource_id);
        delete(path, None).await
    }

    pub(crate) async fn up_resource(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        update(format!("{}/{}/power", self.path(), resource_id), None).await
    }

    pub(crate) async fn down_resource(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        delete(format!("{}/{}/power", self.path(), resource_id), None).await
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

    pub(crate) async fn wait_down(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        let path = format!("{}/{}", self.path(), resource_id);
        wait_resource_down(&path, self.single_name()).await
    }

    pub(crate) async fn wait_delete(&self, resource_id: impl AsRef<str>) -> Result<(), Error> {
        let resource_id = resource_id.as_ref();
        let path = format!("{}/{}", self.path(), resource_id);
        fetch_until_not_found(&path).await
    }
}

// string id and integer id are both is OK in SakuraCloud API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ResourceId {
    String(String),
    Integer(u64),
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Integer(n) => write!(f, "{}", n),
        }
    }
}

impl From<&str> for ResourceId {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<String> for ResourceId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<u64> for ResourceId {
    fn from(n: u64) -> Self {
        Self::Integer(n)
    }
}

// Archive

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArchiveId(pub ResourceId);

impl fmt::Display for ArchiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ArchiveId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArchiveRef {
    #[serde(rename = "ID")]
    id: ArchiveId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Archive, e.to_string()))
    }

    /* commented out because it's not used
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Archive
    }
    */

    pub(crate) fn id(&self) -> &ArchiveId {
        &self.id
    }
}


// Server

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerId(pub ResourceId);

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ServerId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerRef {
    #[serde(rename = "ID")]
    id: ServerId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Server {
    #[serde(rename = "ID")]
    id: ServerId,

    #[serde(rename = "Instance", skip_serializing_if = "Option::is_none")]
    instance: Option<Instance>,

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

    pub(crate) async fn delete(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.delete(server_id.to_string()).await
    } 

    pub(crate) async fn wait_delete(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.wait_delete(server_id.to_string()).await
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

    pub(crate) async fn down(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.down_resource(server_id.to_string()).await
    }

    pub(crate) async fn wait_down(server_id: impl Borrow<ServerId>) -> Result<(), Error> {
        let server_id = server_id.borrow();
        ResourceKind::Server.wait_down(server_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Server, e.to_string()))
    }

    /* commented out because it's not used
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Server
    }
    */

    pub(crate) fn id(&self) -> &ServerId {
        &self.id
    }

    pub(crate) fn instance_status(&self) ->Result<InstanceStatus, Error> {
        if let Some(instance) = &self.instance {
            Ok(instance.status)
        } else {
            Err(Error::ResourceUnknownInstanceStatus)
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerInfo {
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    #[serde(rename = "ServerPlan", skip_serializing_if = "Option::is_none")]
    server_plan: Option<ServerPlanRef>,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "HostName", skip_serializing_if = "Option::is_none")]
    host_name: Option<String>,

    #[serde(rename = "InterfaceDriver", skip_serializing_if = "Option::is_none")]
    interface_driver: Option<InterfaceDriver>,

    // XXX probably used follows only creation time

    #[serde(rename = "ConnectedSwitches", skip_serializing_if = "Option::is_none")]
    connected_switches: Option<Vec<ConnectedSwitch>>,

    #[serde(rename = "WaitDiskMigration", skip_serializing_if = "Option::is_none")]
    wait_disk_migration: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectedSwitch {
    Shared,
    Switch(SwitchRef),
}

impl ServerInfo {
    pub(crate) fn builder() -> ServerInfoBuilder {
        ServerInfoBuilder::new()
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::ServerPlan, e.to_string()))
    }
}

impl Serialize for ConnectedSwitch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        match self {
            Self::Shared => {
                json!({ "Scope": "shared" }).serialize(serializer)
            }
            Self::Switch(switch_ref) => {
                switch_ref.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for ConnectedSwitch {
    fn deserialize<D>(deserializer: D) -> Result<ConnectedSwitch, D::Error> where D: serde::Deserializer<'de> {
        let value = Value::deserialize(deserializer)?;
        if value.is_object() {
            let scope = value.get("Scope").and_then(Value::as_str);
            if scope == Some("shared") {
                Ok(ConnectedSwitch::Shared)
            } else {
                let switch_ref = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(ConnectedSwitch::Switch(switch_ref))
            }
        } else {
            Err(serde::de::Error::custom("invalid value type"))
        }
    }
}

#[derive(Debug)]
pub(crate) struct ServerInfoBuilder {
    name: Option<String>,
    server_plan: Option<ServerPlanRef>,
    description: Option<String>,
    host_name: Option<String>,
    interface_driver: Option<InterfaceDriver>,
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
            interface_driver: None,
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
        self.interface_driver = Some(interface_driver);
        self
    }

    pub(crate) fn connected_switch_ids(mut self, connected_switches: Vec<SwitchId>) -> Self {
        self.connected_switches = Some(connected_switches.into_iter().map(|id| ConnectedSwitch::Switch(SwitchRef { id })).collect());
        self
    }

    /* commented out because it's not used
    pub(crate) fn connect_shared_switch(mut self) -> Self {
        self.connected_switches = Some(vec![ConnectedSwitch::Shared]);
        self
    }
    */

    pub(crate) fn wait_disk_migration(mut self, wait_disk_migration: bool) -> Self {
        self.wait_disk_migration = Some(wait_disk_migration);
        self
    }

    pub(crate) fn build(self) -> ServerInfo {
        ServerInfo {
            name: self.name,
            server_plan: self.server_plan,
            description: self.description,
            host_name: self.host_name,
            interface_driver: self.interface_driver,
            connected_switches: self.connected_switches,
            wait_disk_migration: self.wait_disk_migration,
        }
    }
}


// ServerPlan

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerPlanId(pub ResourceId);

impl fmt::Display for ServerPlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ServerPlanId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerPlanRef {
    #[serde(rename = "ID")]
    id: ServerPlanId,
}

/* commented out because it's not used
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerPlan {
    #[serde(rename = "ID")]
    id: ServerPlanId,
}

impl ServerPlan {
    pub(crate) fn from_value(value: Value) -> Result<ServerPlan, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::ServerPlan, e.to_string()))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::ServerPlan
    }

    pub(crate) fn id(&self) -> &ServerPlanId {
        &self.id
    }
}
*/



// Switch

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SwitchId(pub ResourceId);

impl fmt::Display for SwitchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SwitchId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SwitchRef {
    #[serde(rename = "ID")]
    id: SwitchId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Switch {
    #[serde(rename = "ID")]
    id: SwitchId,

    #[serde(flatten)]
    info: SwitchInfo,
}

impl Switch {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::Switch.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn create(info: SwitchInfo) -> Result<Switch, Error> {
        let info_value = info.to_value()?;
        let res_value = ResourceKind::Switch.create(info_value).await?;
        Switch::from_value(res_value)
    }

    pub(crate) async fn delete(switch_id: impl Borrow<SwitchId>) -> Result<(), Error> {
        let switch_id = switch_id.borrow();
        ResourceKind::Switch.delete(switch_id.to_string()).await
    } 

    pub(crate) async fn wait_delete(switch_id: impl Borrow<SwitchId>) -> Result<(), Error> {
        let switch_id = switch_id.borrow();
        ResourceKind::Switch.wait_delete(switch_id.to_string()).await
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

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Switch, e.to_string()))
    }

    /* commented out because it's not used
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Switch
    }
    */

    pub(crate) fn id(&self) -> &SwitchId {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SwitchInfo {
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl SwitchInfo {
    pub(crate) fn builder() -> SwitchInfoBuilder {
        SwitchInfoBuilder::new()
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Switch, e.to_string()))
    }
}

#[derive(Debug)]
pub(crate) struct SwitchInfoBuilder {
    name: Option<String>,
    description: Option<String>,
}

impl SwitchInfoBuilder {
    fn new() -> Self {
        Self {
            name: None,
            description: None,
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

    pub(crate) fn build(self) -> SwitchInfo {
        SwitchInfo {
            name: self.name,
            description: self.description,
        }
    }
}

// Appliance

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ApplianceId(pub ResourceId);

impl fmt::Display for ApplianceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ApplianceId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Appliance {
    #[serde(rename = "ID")]
    id: ApplianceId,

    #[serde(rename = "Instance", skip_serializing_if = "Option::is_none")]
    instance: Option<Instance>,

    #[serde(flatten)]
    info: ApplianceInfo,
}

impl Appliance {
    pub(crate) async fn get_by_name(name: impl AsRef<str>) -> Result<Option<Self>, Error> {
        let resource_value = ResourceKind::Appliance.search_by_name(name).await?;
        resource_value.map(|resource_value| Self::from_value(resource_value)).transpose()
    }

    pub(crate) async fn create(info: ApplianceInfo) -> Result<Appliance, Error> {
        let info_value = info.to_value()?;
        let res_value = ResourceKind::Appliance.create(info_value).await?;
        Appliance::from_value(res_value)
    }

    pub(crate) async fn update(appliance_id: impl Borrow<ApplianceId>, info: ApplianceInfo) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        let info_value = info.to_value()?;
        ResourceKind::Appliance.update(appliance_id.to_string(), info_value).await
    }

    pub(crate) async fn delete(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.delete(appliance_id.to_string()).await
    } 

    pub(crate) async fn wait_delete(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.wait_delete(appliance_id.to_string()).await
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

    pub(crate) async fn down(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.down_resource(appliance_id.to_string()).await
    }

    pub(crate) async fn wait_down(appliance_id: impl Borrow<ApplianceId>) -> Result<(), Error> {
        let appliance_id = appliance_id.borrow();
        ResourceKind::Appliance.wait_down(appliance_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Appliance, e.to_string()))
    }

    /* commented out because it's not used
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Appliance
    }
    */

    pub(crate) fn id(&self) -> &ApplianceId {
        &self.id
    }

    pub(crate) fn instance_status(&self) ->Result<InstanceStatus, Error> {
        if let Some(instance) = &self.instance {
            Ok(instance.status)
        } else {
            Err(Error::ResourceUnknownInstanceStatus)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ApplianceInfo {
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "Class", skip_serializing_if = "Option::is_none")]
    class: Option<ApplianceClass>,

    #[serde(flatten)]
    class_info: Option<ApplianceClassInfo>,
}

impl ApplianceInfo {
    pub(crate) fn builder() -> ApplianceInfoBuilder {
        ApplianceInfoBuilder::new()
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Appliance, e.to_string()))
    }
}

#[derive(Debug)]
pub(crate) struct ApplianceInfoBuilder {
    name: Option<String>,
    description: Option<String>,
    class: Option<ApplianceClass>,
    class_info: Option<ApplianceClassInfo>,
}

impl ApplianceInfoBuilder {
    fn new() -> Self {
        Self {
            name: None,
            description: None,
            class: None,
            class_info: None,
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

    pub(crate) fn vpc_router(mut self, vpc_router_info: VpcRouterInfo) -> Self {
        self.class = Some(ApplianceClass::VpcRouter);
        self.class_info= Some(ApplianceClassInfo::VpcRouter(vpc_router_info));
        self
    }

    pub(crate) fn vpc_router_info(mut self, vpc_router_info: VpcRouterInfo) -> Self {
        self.class_info= Some(ApplianceClassInfo::VpcRouter(vpc_router_info));
        self
    }

    pub(crate) fn build(self) -> ApplianceInfo {
        ApplianceInfo {
            name: self.name,
            description: self.description,
            class: self.class,
            class_info: self.class_info,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ApplianceClass {
    #[serde(rename = "vpcrouter")]
    VpcRouter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ApplianceClassInfo {

    #[serde(rename = "vpcrouter")]
    VpcRouter(VpcRouterInfo),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VpcRouterInfo {
    #[serde(rename = "Plan", skip_serializing_if = "Option::is_none")]
    plan: Option<VpcRouterPlanRef>,

    #[serde(rename = "Remark", skip_serializing_if = "Option::is_none")]
    remark: Option<Value>,

    #[serde(rename = "Settings", skip_serializing_if = "Option::is_none")]
    settings: Option<Value>,
}

impl VpcRouterInfo {
    pub(crate) fn builder() -> VpcRouterInfoBuilder {
        VpcRouterInfoBuilder::new()
    }

    /* commented out because it's not used
    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Appliance, e.to_string()))
    }
    */
}

#[derive(Debug)]
pub(crate) struct VpcRouterInfoBuilder {
    plan: Option<VpcRouterPlanRef>,
    remark: Option<Value>,
    settings: Option<Value>,
}

impl VpcRouterInfoBuilder {
    fn new() -> Self {
        Self {
            plan: None,
            remark: None,
            settings: None,
        }
    }

    pub(crate) fn plan_id(mut self, plan_id: VpcRouterPlanId) -> Self {
        self.plan = Some(VpcRouterPlanRef { id: plan_id });
        self
    }

    pub(crate) fn remark(mut self, remark: Value) -> Self {
        self.remark = Some(remark);
        self
    }

    pub(crate) fn settings(mut self, settings: Value) -> Self {
        self.settings = Some(settings);
        self
    }

    pub(crate) fn build(self) -> VpcRouterInfo {
        VpcRouterInfo {
            plan: self.plan,
            remark: self.remark,
            settings: self.settings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VpcRouterPlanId(pub ResourceId);

impl VpcRouterPlanId {
    pub(crate) fn new(id: u64) -> Self {
        Self(id.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VpcRouterPlanRef {
    #[serde(rename = "ID")]
    id: VpcRouterPlanId,
}


// Disk

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskId(pub ResourceId);

impl fmt::Display for DiskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DiskId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub(crate) async fn delete(disk_id: impl Borrow<DiskId>) -> Result<(), Error> {
        let disk_id = disk_id.borrow();
        ResourceKind::Disk.delete(disk_id.to_string()).await
    } 

    pub(crate) async fn wait_delete(disk_id: impl Borrow<DiskId>) -> Result<(), Error> {
        let disk_id = disk_id.borrow();
        ResourceKind::Disk.wait_delete(disk_id.to_string()).await
    }

    pub(crate) async fn wait_available(disk_id: impl Borrow<DiskId>) -> Result<(), Error> {
        let disk_id = disk_id.borrow();
        ResourceKind::Disk.wait_available(disk_id.to_string()).await
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, Error> {
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Disk, e.to_string()))
    }

    /*
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Disk
    }
    */

    pub(crate) fn id(&self) -> &DiskId {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskInfo {
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "Plan", skip_serializing_if = "Option::is_none")]
    plan: Option<DiskPlanRef>,

    #[serde(rename = "SourceArchive", skip_serializing_if = "Option::is_none")]
    source_archive: Option<ArchiveRef>,

    #[serde(rename = "SizeMB", skip_serializing_if = "Option::is_none")]
    size_mb: Option<u64>,

    #[serde(rename = "Connection", skip_serializing_if = "Option::is_none")]
    connection: Option<DiskConnection>,

    #[serde(rename = "Server", skip_serializing_if = "Option::is_none")]
    server: Option<ServerRef>,
}

impl DiskInfo {
    pub(crate) fn builder() -> DiskInfoBuilder {
        DiskInfoBuilder::new() 
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Disk, e.to_string()))
    }
}

#[derive(Debug)]
pub(crate) struct DiskInfoBuilder {
    name: Option<String>,
    description: Option<String>,
    plan: Option<DiskPlanRef>,
    source_archive: Option<ArchiveRef>,
    size_mb: Option<u64>,
    connection: Option<DiskConnection>,
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
            connection: None,
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
        self.connection = Some(connection);
        self
    }

    pub(crate) fn server_id(mut self, server_id: ServerId) -> Self {
        self.server = Some(ServerRef { id: server_id });
        self
    }

    pub(crate) fn build(self) -> DiskInfo {
        DiskInfo {
            name: self.name,
            description: self.description,
            plan: self.plan,
            source_archive: self.source_archive,
            size_mb: self.size_mb,
            connection: self.connection,
            server: self.server,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DiskConnection {
    #[serde(rename = "virtio")]
    Virtio,

    #[serde(rename = "ide")]
    Ide,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskConfig {
    #[serde(rename = "Password", skip_serializing_if = "Option::is_none")]
    password: Option<String>,

    #[serde(rename = "HostName", skip_serializing_if = "Option::is_none")]
    host_name: Option<String>,

    #[serde(rename = "SSHKeys", skip_serializing_if = "Option::is_none")]
    ssh_keys: Option<Vec<SshPublicKeyRef>>,

    #[serde(rename = "ChangePartitionUUID", skip_serializing_if = "Option::is_none")]
    change_partition_uuid: Option<bool>,

    #[serde(rename = "DisablePWAuth", skip_serializing_if = "Option::is_none")]
    disable_pw_auth: Option<bool>,

    #[serde(rename = "UserIPAddress", skip_serializing_if = "Option::is_none")]
    user_ip_address: Option<Ipv4Addr>,

    #[serde(rename = "UserSubnet", skip_serializing_if = "Option::is_none")]
    user_subnet: Option<Ipv4Net>,

    #[serde(rename = "EnableDHCP", skip_serializing_if = "Option::is_none")]
    enable_dhcp: Option<bool>,

    #[serde(rename = "Notes", skip_serializing_if = "Option::is_none")]
    notes: Option<Vec<NoteRef>,>
}

impl DiskConfig {
    pub(crate) fn builder() -> DiskConfigBuilder {
        DiskConfigBuilder::new() 
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::Disk, e.to_string()))
    }
}

#[derive(Debug)]
pub(crate) struct DiskConfigBuilder {
    password: Option<String>,
    host_name: Option<String>,
    ssh_keys: Option<Vec<SshPublicKeyRef>>,
    change_partition_uuid: Option<bool>,
    disable_pw_auth: Option<bool>,
    user_ip_address: Option<Ipv4Addr>,
    user_subnet: Option<Ipv4Net>,
    enable_dhcp: Option<bool>,
    notes: Option<Vec<NoteRef>>,
}

impl DiskConfigBuilder {
    fn new() -> Self {
        Self {
            password: None,
            host_name: None,
            ssh_keys: None,
            change_partition_uuid: None,
            disable_pw_auth: None,
            user_ip_address: None,
            user_subnet: None,
            enable_dhcp: None,
            notes: None
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
        self.ssh_keys = Some(ssh_key_ids.into_iter().map(|id| SshPublicKeyRef { id }).collect());
        self
    }

    pub(crate) fn change_partition_uuid(mut self, change_partition_uuid: bool) -> Self {
        self.change_partition_uuid = Some(change_partition_uuid);
        self
    }

    pub(crate) fn disable_pw_auth(mut self, disable_pw_auth: bool) -> Self {
        self.disable_pw_auth = Some(disable_pw_auth);
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
        self.enable_dhcp = Some(enable_dhcp);
        self
    }
    
    pub(crate) fn note_id_and_variables_pairs(mut self, note_ids: Vec<(NoteId, Value)>) -> Self {
        self.notes = Some(note_ids.into_iter().map(|(id, variables)| NoteRef::new(id, variables)).collect());
        self
    }

    pub(crate) fn build(self) -> DiskConfig {
        DiskConfig {
            password: self.password,
            host_name: self.host_name,
            ssh_keys: self.ssh_keys,
            change_partition_uuid: self.change_partition_uuid,
            disable_pw_auth: self.disable_pw_auth,
            user_ip_address: self.user_ip_address,
            user_subnet: self.user_subnet,
            enable_dhcp: self.enable_dhcp,
            notes: self.notes,
        }
    }
}

// DiskPlan

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskPlanId(pub ResourceId);

impl fmt::Display for DiskPlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DiskPlanId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiskPlanRef {
    #[serde(rename = "ID")]
    id: DiskPlanId,
}

/* comment out unused
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::DiskPlan, e.to_string()))
    }

    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::DiskPlan
    }

    pub(crate) fn id(&self) -> &DiskPlanId {
        &self.id
    }
}
*/


// SshPublicKey

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SshPublicKeyId(pub ResourceId);

impl fmt::Display for SshPublicKeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SshPublicKeyId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SshPublicKeyRef {
    #[serde(rename = "ID")]
    id: SshPublicKeyId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SshPublicKey {
    #[serde(rename = "ID")]
    id: SshPublicKeyId,

    #[serde(flatten)]
    info: SshPublicKeyInfo,
}

impl SshPublicKey {
    pub(crate) fn public_key(&self) -> &str {
        &self.info.public_key.as_ref().expect("must be set")
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
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::SshPublicKey, e.to_string()))
    }

    /* commented out because it's not used
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::SshPublicKey
    }
    */

    pub(crate) fn id(&self) -> &SshPublicKeyId {
        &self.id
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SshPublicKeyInfo {
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(rename = "PublicKey", skip_serializing_if = "Option::is_none")]
    public_key: Option<String>,
}

impl SshPublicKeyInfo {
    pub(crate) fn builder() -> SshPublicKeyInfoBuilder {
        SshPublicKeyInfoBuilder::new()
    }

    pub(crate) fn to_value(&self) -> Result<Value, Error> {
        serde_json::to_value(self).map_err(|e| Error::ResourceSerializationFailed(ResourceKind::SshPublicKey, e.to_string()))
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

    pub(crate) fn build(self) -> SshPublicKeyInfo {
        SshPublicKeyInfo {
            name: self.name,
            description: self.description,
            public_key: self.public_key,
        }
    }
}

// Note

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NoteId(pub ResourceId);

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for NoteId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        serde_json::from_value(value).map_err(|e| Error::ResourceDeserializationFailed(ResourceKind::Note, e.to_string()))
    }

    /* commented out because it's not used
    pub(crate) fn kind() -> ResourceKind {
        ResourceKind::Note
    }
    */

    pub(crate) fn id(&self) -> &NoteId {
        &self.id
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IpAddressRef {
    #[serde(rename = "IPAddress")]
    ip_address: Ipv4Addr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SingleLineIpv4Net(pub(crate) String);

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Instance {
    #[serde(rename = "Status")]
    status: InstanceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum InstanceStatus {
    #[serde(rename = "cleaning")]
    Cleaning,

    #[serde(rename = "up")]
    Up,

    #[serde(rename = "down")]
    Down,
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

async fn wait_resource_down(path: impl AsRef<str>, resource_name: impl AsRef<str>) -> Result<(), Error> {
    wait_resource_status(path, resource_name,
        |res| res["Instance"]["Status"].as_str().map(|s| s.to_string()),
        ["up", "cleaning"].into_iter().collect(),
        ["down"].into_iter().collect(),
        [].into_iter().collect()).await
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

async fn fetch_until_not_found(path: impl AsRef<str>) -> Result<(), Error> {
    let path = path.as_ref();
    loop {
        match request_api(Method::GET, path, &None, &None).await {
            Ok(_) => {},    
            Err(Error::ApiNotFound(..)) => return Ok(()),
            Err(e) => return Err(e),
        }
        sleep(Duration::from_secs(2)).await;
    }
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

async fn delete(path: impl AsRef<str>, body: Option<Value>) -> Result<(), Error> {
    let _ = request_api_for_resource(Method::DELETE, path, None, body).await?;
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
    log::trace!("START API REQUEST: method={:?}, path={}, query={}, body={}", method, path, serde_json::to_string_pretty(&query).unwrap_or_default(), serde_json::to_string_pretty(&body).unwrap_or_default());

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
            return Err(Error::RequestFailed(e.to_string(), path.to_string(), body.clone()));
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
            log::trace!("ERROR API REQUEST: response={}", res.text().await.unwrap_or_default());
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
                    return Err(Error::ApiUnknownStatusCode(status_code.as_u16(), path.to_string(), body.clone()));
                },
            }
        },
    }

    let value = res.json().await.map_err(|e| Error::InvalidResponseJson(e.to_string(), path.to_string(), body.clone()))?;
    log::trace!("END API REQUEST: value={}", serde_json::to_string_pretty(&value).unwrap_or_default());
    Ok(value)
}

// test
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_json() {
        let name = "NAME".to_string();
        let server_id = ServerId("SERVER_ID".into());
        let server_plan_id = ServerPlanId("SERVER_PLAN_ID".into());
        let switch_id = SwitchId("SWITCH_ID".into());

        let server_info = ServerInfo::builder()
            .name(name.clone())
            .server_plan(server_plan_id.clone())
            .description(name.clone())
            .host_name(name.clone())
            .connected_switch_ids(vec![switch_id.clone()])
            .interface_driver(InterfaceDriver::Virtio)
            .wait_disk_migration(true)
            .build();

        assert_eq!(json!({ "Server": &server_info }), json!({
            "Server": {
                "Name": "NAME",
                "ServerPlan": { "ID": "SERVER_PLAN_ID" },
                "Description": "NAME",
                "HostName": "NAME",
                "ConnectedSwitches": [{ "ID": "SWITCH_ID" }],
                "InterfaceDriver": "virtio",
                "WaitDiskMigration": true,
            },
        }));

        let server = Server::from_value(json!({
            "ID": "SERVER_ID",
            "Name": "NAME",
            "ServerPlan": { "ID": "SERVER_PLAN_ID" },
            "Description": "NAME",
            "HostName": "NAME",
            "ConnectedSwitches": [{ "ID": "SWITCH_ID" }],
            "InterfaceDriver": "virtio",
            "WaitDiskMigration": true,
            "Instance": { "Status": "up" },
            "UnknowField": "UNKNOWN",
        })).unwrap();
        assert_eq!(server.id(), &server_id);

        assert_eq!(server.info, server_info);
        let mut server_info = ServerInfo::builder()
            .name(name.clone())
            .server_plan(server_plan_id.clone())
            .build();
        server_info.connected_switches = Some(vec![ConnectedSwitch::Shared]);

        assert_eq!(json!({ "Server": &server_info }), json!({
            "Server": {
                "Name": "NAME",
                "ServerPlan": { "ID": "SERVER_PLAN_ID" },
                "ConnectedSwitches": [ { "Scope": "shared" } ],
            },
        }));

        let server = Server::from_value(json!({
            "ID": "SERVER_ID",
            "Name": "NAME",
            "ServerPlan": { "ID": "SERVER_PLAN_ID" },
            "ConnectedSwitches": [ { "Scope": "shared" } ],
            "UnknowField": "UNKNOWN",
        })).unwrap();

        assert_eq!(server.id(), &server_id);
        assert_eq!(server.info, server_info);
    }

    #[test]
    fn server_disk_json() {
        let disk_id = DiskId("DISK_ID".into());
        let name = "NAME".to_string();
        let description = "DESCRIPTION".to_string();
        let disk_plan_id = DiskPlanId(111.into());
        let archive_id = ArchiveId("ARCHIVE_ID".into());
        let server_id = ServerId("SERVER_ID".into());
        let ssh_public_key_id = SshPublicKeyId("SSH_PUBLIC_KEY_ID".into());
        let password = "PASSWORD".to_string();
        let note_id = NoteId("NOTE_ID".into());

        let info = DiskInfo::builder()
            .name(name.clone())
            .description(description.clone())
            .plan_id(disk_plan_id.clone())
            .source_archive_id(archive_id.clone())
            .size_mb(222)
            .connection(DiskConnection::Virtio)
            .server_id(server_id.clone())
            .build();

        let config = DiskConfig::builder()
            .host_name(name.clone())
            .ssh_key_ids(vec![ssh_public_key_id.clone()])
            .user_ip_address(Ipv4Addr::new(11, 11, 11, 11))
            .user_subnet(Ipv4Net::new(Ipv4Addr::new(11, 11, 11, 1), 24))
            .change_partition_uuid(false)
            .enable_dhcp(false)
            .note_id_and_variables_pairs(vec![
                (note_id.clone(), json!({ "usacloud": false, "updatepackage": true }))
            ])
            .disable_pw_auth(false)
            .password(password.clone())
            .build();

        assert_eq!(serde_json::to_value(json!({ "Disk": &info, "Config": &config })).unwrap(), json!({
            "Disk": {
                "Name": "NAME",
                "Description": "DESCRIPTION",
                "Plan": { "ID": 111 },
                "SourceArchive": { "ID": "ARCHIVE_ID" },
                "SizeMB": 222,
                "Connection": "virtio",
                "Server": { "ID": "SERVER_ID" },
            },
            "Config":{
                "Password": "PASSWORD",
                "HostName": "NAME",
                "SSHKeys": [{"ID": "SSH_PUBLIC_KEY_ID"}],
                "ChangePartitionUUID": false,
                "DisablePWAuth": false,
                "UserIPAddress":"11.11.11.11",
                "UserSubnet": { "DefaultRoute": "11.11.11.1", "NetworkMaskLen": 24 },
                "EnableDHCP": false,
                "Notes":[ { "ID": "NOTE_ID", "Variables": { "usacloud": false, "updatepackage": true } } ]
            },
        }));


        let disk = Disk::from_value(json!({
            "ID": "DISK_ID",
            "Name": "NAME",
            "Description": "DESCRIPTION",
            "Plan": { "ID": 111 },
            "SourceArchive": { "ID": "ARCHIVE_ID" },
            "SizeMB": 222,
            "Connection": "virtio",
            "Server": { "ID": "SERVER_ID" },
            "UnknowField": "UNKNOWN",
        })).unwrap();

        assert_eq!(disk.id(), &disk_id);
        assert_eq!(disk.info, info);
    }

    #[test]
    fn ssh_public_key_json() {
        let id = SshPublicKeyId("SSH_PUBLIC_KEY_ID".into());
        let name = "NAME".to_string();
        let description = "DESCRIPTION".to_string();
        let public_key = "PUBLIC_KEY".to_string();

        let info = SshPublicKeyInfo::builder()
            .name(name.clone())
            .description(description.clone())
            .public_key(public_key.clone())
            .build();

        assert_eq!(serde_json::to_value(json!({ "SSHKey": &info })).unwrap(), json!({
            "SSHKey": {
                "Name": "NAME",
                "Description": "DESCRIPTION",
                "PublicKey": "PUBLIC_KEY",
            },
        }));

        let ssh_public_key = SshPublicKey::from_value(json!({
            "ID": "SSH_PUBLIC_KEY_ID",
            "Name": "NAME",
            "Description": "DESCRIPTION",
            "PublicKey": "PUBLIC_KEY",
            "UnknowField": "UNKNOWN",
        })).unwrap();

        assert_eq!(ssh_public_key.id(), &id);
        assert_eq!(ssh_public_key.info, info);
    }

    #[test]
    fn switch_json() {
        let id = SwitchId("SWITCH_ID".into());
        let name = "NAME".to_string();
        let description = "DESCRIPTION".to_string();

        let info = SwitchInfo::builder()
            .name(name.clone())
            .description(description.clone())
            .build();

        assert_eq!(serde_json::to_value(json!({ "Switch": &info })).unwrap(), json!({
            "Switch": {
                "Name": "NAME",
                "Description": "DESCRIPTION",
            },
        }));

        let switch = Switch::from_value(json!({
            "ID": "SWITCH_ID",
            "Name": "NAME",
            "Description": "DESCRIPTION",
            "UnknowField": "UNKNOWN",
        })).unwrap();

        assert_eq!(switch.id(), &id);
        assert_eq!(switch.info, info);
    }
}

