use std::{fmt::Write as _, net::Ipv4Addr, path::Path, time::{Duration, Instant}};
use once_cell::sync::Lazy;
use serde::{Serialize};
use tokio::time::sleep;

use crate::{
    service_env::{
        CONFIG,
        PRIMARY_SERVER_FORWARDED_PORT,
    },
    ssh::{
        self,
        Session,
    },
};

static TEMPLATE_ENGINE: Lazy<upon::Engine> = Lazy::new(|| {
    let mut engine = upon::Engine::new();
    engine.set_default_formatter(&escape_shell);
    engine.add_template(ServiceScript::RootSetup.as_str(), include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/service_script/root-setup.zsh"))).unwrap();
    engine.add_template(ServiceScript::UserSetup.as_str(), include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/service_script/user-setup.zsh"))).unwrap();
    engine
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum Error {
    RenderError(String),
    IllegallyStopped,
    Failed,
    Timeout,
    SshError(ssh::Error),
}

impl From<upon::Error> for Error {
    fn from(e: upon::Error) -> Self {
        Error::RenderError(e.to_string())
    }
}

impl From<ssh::Error> for Error {
    fn from(e: ssh::Error) -> Self {
        Error::SshError(e)
    }
}

pub(crate) enum ServiceScript {
    RootSetup,
    UserSetup,
}

impl ServiceScript {
    pub(crate) async fn prepare_for_server(ip: Ipv4Addr, user: impl AsRef<str>, pubkey_path: impl AsRef<Path>) -> Result<(), Error> {
        let server_info = &CONFIG.server;
        let root_setup_script = Self::RootSetup.render(&server_info)?;
        let user_setup_script = Self::UserSetup.render(&server_info)?;
        let root_setup_script = root_setup_script.as_bytes();
        let user_setup_script = user_setup_script.as_bytes();

        let session = Session::connect(ip, PRIMARY_SERVER_FORWARDED_PORT, user, pubkey_path).await?;
        session.put_file("root-setup.zsh", root_setup_script).await?;
        session.put_file("user-setup.zsh", user_setup_script).await?;
        session.put_file("root_setup_not_yet_started_once", &b""[..]).await?;
        session.put_file("root_setup_not_yet_finished_once", &b""[..]).await?;
        session.put_file("root_setup_not_yet_success_once", &b""[..]).await?;
        Ok(())
    }

    pub(crate) async fn wait_for_done(ip: Ipv4Addr, user: impl AsRef<str>, pubkey_path: impl AsRef<Path>) -> Result<(), Error> {
        let session = Session::connect(ip, PRIMARY_SERVER_FORWARDED_PORT, user, pubkey_path).await?;
        let start_waiting = Instant::now();
        loop {
            let exists_process = session.process_exists("root-setup.zsh").await?;
            let started = !session.file_exists("root_setup_not_yet_started_once").await?;
            let finished = session.file_exists("root_setup_not_yet_finished_once").await?;
            let success = session.file_exists("root_setup_not_yet_success_once").await?;

            // プロセスが終わっていて、プロセスを開始した痕跡があるまでループ
            if !exists_process && started {
                if !finished {
                    // 正常に終了できてない
                    return Err(Error::IllegallyStopped);
                }
                if !success {
                    // 正常に終了できていない
                    return Err(Error::Failed);
                }
                break;
            }

            if start_waiting.elapsed() > Duration::from_secs(60 * 10) {
                return Err(Error::Timeout);
            }

            sleep(Duration::from_secs(5)).await;
        }
        Ok(())
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ServiceScript::RootSetup => "root_setup",
            ServiceScript::UserSetup => "user_setup",
        }
    }

    pub(crate) fn render(&self, data: impl Serialize) -> Result<String, Error> {
        let script = TEMPLATE_ENGINE.template(self.as_str()).render(data).to_string()?;
        Ok(script)
    }
}

fn escape_shell(formatter: &mut upon::fmt::Formatter<'_>, value: &upon::Value) -> upon::fmt::Result {
    match value {
        upon::Value::None => return Err("Value::None is not supported in shell script template".into()),
        upon::Value::String(s) => formatter.write_str(&shell_escape::escape(s.into()))?,
        upon::Value::Bool(b) => return Err(format!("Value::Bool({}) is not supported in shell script template, because what boolean is depends on syntaxt context", b).into()),
        upon::Value::Integer(i) => formatter.write_str(&i.to_string())?,
        upon::Value::Float(f) => formatter.write_str(&f.to_string())?,
        upon::Value::List(l) => {
            formatter.write_str("(")?;
            for v in l.iter() {
                if let upon::Value::List(_) | upon::Value::Map(_) = v {
                    return Err("nested list or map is not supported in shell script template".into());
                }
                escape_shell(formatter, v)?;
                formatter.write_str(" ")?;
            }
            formatter.write_str(")")?;
        },
        upon::Value::Map(m) => {
            formatter.write_str("(")?;
            for (k, v) in m.iter() {
                if let upon::Value::List(_) | upon::Value::Map(_) = v {
                    return Err("nested list or map is not supported in shell script template".into());
                }
                formatter.write_str(&shell_escape::escape(k.into()))?;
                formatter.write_str(" ")?;
                escape_shell(formatter, v)?;
                formatter.write_str(" ")?;
            }
            formatter.write_str(")")?;
        },
    };
    Ok(())
}

