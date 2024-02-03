use std::env;
use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use url::Url;
use serde_json::{Value, json, to_string_pretty};
use reqwest::{Method, StatusCode};

#[derive(Debug, Parser)]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    ShowEnv(ShowEnvArgs),
    CreateEnv(CreateEnvArgs),
}

#[derive(Debug, Parser)]
struct ShowEnvArgs {
    #[arg(short, long)]
    prefix: String,
}

#[derive(Debug, Parser)]
struct CreateEnvArgs {
    #[arg(short, long)]
    prefix: String,
}

#[derive(Debug)]
enum Error {
    RequestFailed(reqwest::Error, String, Value),
    InvalidResponseJson(reqwest::Error, String, Value),
    ApiBadRequest(String, Value),
    ApiUnauthorized(String, Value),
    ApiForbidden(String, Value),
    ApiNotFound(String, Value),    
    ApiMethodNotAllowed(String, Value),
    ApiNotAcceptable(String, Value),
    ApiRequestTimeout(String, Value),
    ApiConflict(String, Value),
    ApiLengthRequired(String, Value),
    ApiPayloadTooLarge(String, Value),
    ApiUnsupportedMediaType(String, Value),
    ApiInternalServerError(String, Value),
    ApiServiceUnavailable(String, Value),
    ApiUnknownStatusCode(StatusCode, String, Value),
    SearchApiInvalidTotalCount(String, Value),
    SearchApiInvalidIndexFrom(Option<u64>, String, Value),
    SearchApiInvalidResourceCount(String, Value),
    SearchApiInvalidResourceArray(Value, String, Value),
    ResourceApiInvalidStatusBoolean(String, Value),
    ResourceApiInvalidStatusFalse(String, Value),
    ResourceApiInvalidResourceObject(String, Value),
    TooManyResources(String, usize),
    ResourceNotFound(String),
}

static ACCESS_TOKEN: Lazy<String> = Lazy::new(|| { env::var("SACLOUD_ACCESS_TOKEN").unwrap() });
static SECRET_TOKEN: Lazy<String> = Lazy::new(|| { env::var("SACLOUD_SECRET_TOKEN").unwrap() });
static API_BASE_URL: Lazy<Url> = Lazy::new(|| { Url::parse(format!("https://secure.sakura.ad.jp/cloud/zone/{}/api/cloud/1.1/", env::var("SACLOUD_ZONE").unwrap()).as_str()).unwrap() });

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();
    match args.cmd {
        Cmd::ShowEnv(args) => show_env(args.prefix).await?,
        Cmd::CreateEnv(args) => create_env(args.prefix).await?,
    };
    Ok(())
}

async fn show_env(prefix: impl AsRef<str>) -> Result<(), Error> {
    let prefix = prefix.as_ref();
    let server = search_primary_server(prefix).await?;
    println!("{}", to_string_pretty(&server).unwrap());
    todo!();
}

async fn create_env(prefix: impl AsRef<str>) -> Result<(), Error> {
    let prefix = prefix.as_ref();
    let _router_name = create_vpc_router(prefix).await?;
    let _switch_name = create_switch(prefix).await?;
    let disk_name = create_disk(prefix).await?;
    let server_name = create_primary_server(prefix).await?;
    connect_disk_to_server(&disk_name, &server_name).await?;
    start_server(&server_name).await?;
    Ok(())
}

async fn search_primary_server(prefix: impl AsRef<str>) -> Result<Value, Error> {
    let prefix = prefix.as_ref();
    let name = format!("{}-server", prefix);
    let servers = request_search_api("server", "Servers", Some(json!({ "Name": name })), None, None, 50).await?;
    if servers.len() < 1 {
        return Err(Error::ResourceNotFound("server".to_string()));
    }
    if servers.len() > 1 {
        return Err(Error::TooManyResources("server".to_string(), servers.len()));
    }
    Ok(servers[0].clone())
}

async fn create_primary_server(prefix: impl AsRef<str>) -> Result<String, Error> {
    let name = format!("{}-server", prefix.as_ref());
    let req_body = todo!();
    request_create_api("server", &req_body).await?;
    Ok(name)
}

async fn create_disk(prefix: impl AsRef<str>) -> Result<String, Error> {
    let name = format!("{}-disk", prefix.as_ref());
    let req_body = todo!();
    request_create_api("disk", &req_body).await?;
    Ok(name)
}

async fn connect_disk_to_server(disk_name: impl AsRef<str>, server_name: impl AsRef<str>) -> Result<(), Error> {
    todo!();
}

async fn start_server(server_name: impl AsRef<str>) -> Result<(), Error> {
    todo!();
}

async fn create_switch(prefix: impl AsRef<str>) -> Result<String, Error> {
    let name = format!("{}-swich", prefix.as_ref());
    let req_body = json!({
        "Switch": {
            "Name": name.clone(),
            "Description": name.clone(),
        },
    });
    request_create_api("switch", &req_body).await?;
    Ok(name)
}

async fn create_vpc_router(prefix: impl AsRef<str>) -> Result<String, Error> {
    let name = format!("{}-vpcrouter", prefix.as_ref());
    let req_body = json!({
        "Appliance": {
            "Class": "vpcrouter",
            "Name": name.clone(),
            "Description": name.clone(),
            "Plan": { "ID": 1 },
            "Remark": {
                "Servers": [ {} ],
                "Switch": { "Scope": "shared" }
            },
            "Settings": {
                "Interfaces": [
                    null,
                    { "IPAddress": [ "192.168.2.1" ], "NetworkMaskLen": 24, },
                ],
                "PortForwarding": {
                    "Config": [
                        { "Protocol": "tcp", "GlobalPort": "10022", "PrivateAddress": "192.168.2.2", "PrivatePort": "22" },
                    ],
                    "Enabled": "True",
                },
            },
        },
    });
    request_create_api("appliance", &req_body).await?;
    Ok(name)
}

async fn request_create_api(path: impl AsRef<str>, body: &Value) -> Result<(), Error> {
    let _ = request_resource_api(Method::POST, path, body, None, true, false).await?;
    Ok(())
}

async fn request_fetch_api(path: impl AsRef<str>, body: &Value, resource_name: impl AsRef<str>) -> Result<Value, Error> {
    request_resource_api(Method::GET, path, body, Some(resource_name.as_ref()), true, false).await
}

async fn request_update_api(path: impl AsRef<str>, body: &Value) -> Result<(), Error> {
    let _ = request_resource_api(Method::PUT, path, body, None, false, true).await?;
    Ok(())
}

async fn request_delete_api(path: impl AsRef<str>, body: &Value) -> Result<(), Error> {
    let _ = request_resource_api(Method::DELETE, path, body, None, true, true).await?;
    Ok(())
}

async fn request_resource_api(method: Method, path: impl AsRef<str>, body: &Value, resource_name: Option<&str>, needs_to_check_ok_status: bool, needs_to_check_success_status: bool) -> Result<Value, Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    let mut value = request_api(method, path, body).await?;
    if needs_to_check_ok_status {
        let Some(is_ok) = value["is_ok"].as_bool() else {
            return Err(Error::ResourceApiInvalidStatusBoolean(path.to_string(), body.clone()));
        };
        if !is_ok {
            return Err(Error::ResourceApiInvalidStatusFalse(path.to_string(), body.clone()));
        }
    }
    if needs_to_check_success_status {
        let Some(is_success) = value["Success"].as_bool() else {
            return Err(Error::ResourceApiInvalidStatusBoolean(path.to_string(), body.clone()));
        };
        if !is_success {
            return Err(Error::ResourceApiInvalidStatusFalse(path.to_string(), body.clone()));
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

async fn request_search_api(path: impl AsRef<str>, resource_name: impl AsRef<str>, filter: Option<Value>, sort: Option<Value>, other: Option<Value>, page_count: u64) -> Result<Vec<Value>, Error> {
    let path = path.as_ref();
    let resource_name = resource_name.as_ref();
    let mut result_resources = Vec::new();
    let mut index_from = 0;
    let body = if let Some(other) = other {
        other
    } else {
        json!({})
    };
    loop {
        let mut body = body.clone();
        body["From"] = Value::from(index_from);
        body["Count"] = Value::from(page_count);
        if let Some(filter) = filter.clone() {
            body["Filter"] = filter;
        }
        if let Some(sort) = sort.clone() {
            body["Sort"] = sort;
        }

        let value = request_api(Method::GET, path, &body).await?;
        let Some(total) = value["Total"].as_u64() else {
            return Err(Error::SearchApiInvalidTotalCount(path.to_string(), body.clone()));
        };
        let Some(response_index_from) = value["From"].as_u64() else {
            return Err(Error::SearchApiInvalidIndexFrom(None, path.to_string(), body.clone()));
        };

        if index_from != response_index_from {
            return Err(Error::SearchApiInvalidIndexFrom(Some(response_index_from), path.to_string(), body.clone()));
        }

        let Some(count) = value["Count"].as_u64() else {
            return Err(Error::SearchApiInvalidResourceCount(path.to_string(), body.clone()));
        };

        let Some(resources) = value[resource_name].as_array() else {
            return Err(Error::SearchApiInvalidResourceArray(value, path.to_string(), body.clone()));
        };
        result_resources.extend(resources.to_vec());

        if index_from + count >= total {
            break;
        }

        index_from += count;
    };
    Ok(result_resources)
}

async fn request_api(method: Method, path: impl AsRef<str>, body: &Value) -> Result<Value, Error> {
    let path = path.as_ref();
    log::trace!("START API REQUEST: method={:?}, path={}, body={}", method, path, body);

    let client = reqwest::Client::new();
    let res = client.request(method, API_BASE_URL.join(path).expect("must be valid url"))
        .basic_auth(&*ACCESS_TOKEN, Some(&*SECRET_TOKEN))
        .json(&body)
        .send()
        .await;
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
            log::trace!("ERROR API REQUEST: status={}", status_code);
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

