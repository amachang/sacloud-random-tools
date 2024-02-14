use std::{time::Duration, path::Path, net::Ipv4Addr, time::Instant, sync::Arc};
use shell_escape::unix::escape;
use openssh::{self, SessionBuilder, Stdio, KnownHosts};
use openssh_sftp_client::{self, Sftp};
use openssh_sftp_protocol_error::ErrorCode as SftpErrorKind;
use tokio::{io::{AsyncRead, AsyncReadExt, BufReader, AsyncBufReadExt}, time::{timeout, interval}, net::TcpStream};
use serde::Serialize;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum Error {
    IoError(String),
    OpensshError(String, String),
    OpensshSftpError(String, String),
    PsCommandNoLineOutput,
    CouldntTakeRemoteProcessStdin,
    CouldntTakeRemoteProcessStdout,
    ParseFailedPsOutputLine,
    CouldntGetRemoteFileType,
    PathExistsButNotFile(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e.to_string())
    }
}

impl From<openssh::Error> for Error {
    fn from(e: openssh::Error) -> Self {
        Error::OpensshError(e.to_string(), format!("{:?}", e))
    }
}

impl From<openssh_sftp_client::Error> for Error {
    fn from(e: openssh_sftp_client::Error) -> Self {
        Error::OpensshSftpError(e.to_string(), format!("{:?}", e))
    }
}

pub(crate) struct Session {
    session: Arc<openssh::Session>,
    _sftp_process: openssh::Child<Arc<openssh::Session>>, // for keeping the process alive
    sftp: Sftp,
}

impl Session {
    pub(crate) async fn connect(ip: Ipv4Addr, port: u16, user: impl AsRef<str>, pubkey_path: impl AsRef<Path>) -> Result<Self, Error> {
        let pubkey_path = pubkey_path.as_ref();
        let user = user.as_ref();

        log::trace!("[SSH] connecting to server...: {}:{}", ip, port);
        let start_time = Instant::now();
        let mut interval = interval(Duration::from_secs(20));
        let session = loop {
            log::trace!("[SSH] waiting for ssh to be connectable...: {}:{}", ip, port);
            wait_for_ssh_connectable(ip, port).await?;

            let session = SessionBuilder::default()
                .user(user.to_string())
                .port(port)
                .keyfile(pubkey_path)
                .connect_timeout(Duration::from_secs(10))
                .known_hosts_check(KnownHosts::Accept)
                .server_alive_interval(Duration::from_secs(60))
                .connect_mux(ip.to_string())
                .await;
            match session {
                Ok(session) => break Arc::new(session),
                Err(e) => {
                    log::trace!("[SSH] couldn't connect to server: {} {} {} {}", ip.to_string(), port, user, pubkey_path.display());
                    log::trace!("[SSH] error: {}", e);
                    if start_time.elapsed() > Duration::from_secs(60 * 5) {
                        return Err(e.into());
                    }
                    log::trace!("[SSH] retrying in 20 seconds...");
                    interval.tick().await;
                }
            }
        };

        log::trace!("[SSH] starting sftp subsystem...");
        let mut sftp_process = openssh::Session::to_subsystem(session.clone(), "sftp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .await?;

        let sftp = Sftp::new(
            sftp_process.stdin().take().ok_or(Error::CouldntTakeRemoteProcessStdin)?,
            sftp_process.stdout().take().ok_or(Error::CouldntTakeRemoteProcessStdout)?,
            Default::default(),
        ).await?;

        log::trace!("[SSH] connected to server: {}:{}", ip, port);
        Ok(Self {
            session,
            _sftp_process: sftp_process,
            sftp,
        })
    }

    pub(crate) async fn put_file(&self, remote_path: impl AsRef<Path>, data: impl AsyncRead + Unpin) -> Result<(), Error> {
        let remote_path = remote_path.as_ref();
        log::trace!("[SSH] putting file...: {}", remote_path.display());
        let mut remote_file = self.sftp.create(remote_path).await?;
        log::trace!("[SSH] created remote file: {}", remote_path.display());

        log::trace!("[SSH] copying data to remote file...");
        let mut buf = [0; 4096];
        let mut data = BufReader::new(data);
        loop {
            log::trace!("[SSH] reading data...");
            let n = data.read(&mut buf[..]).await?;
            log::trace!("[SSH] read {} bytes", n);

            if n == 0 {
                break;
            }

            log::trace!("[SSH] writing data...");
            remote_file.write_all(&buf[..n]).await?;
            log::trace!("[SSH] wrote {} bytes", n);
        };
        log::trace!("[SSH] syncing remote file...");
        remote_file.sync_all().await?;
        log::trace!("[SSH] done syncing remote file");

        log::trace!("[SSH] put file: {}", remote_path.display());
        Ok(())
    }

    pub(crate) async fn file_exists(&self, remote_path: impl AsRef<Path>) -> Result<bool, Error> {
        let remote_path = remote_path.as_ref();
        log::trace!("[SSH] checking file exists...: {}", remote_path.display());
        let metadata = match self.sftp.fs().metadata(remote_path).await {
            Ok(metadata) => metadata,
            Err(openssh_sftp_client::Error::IOError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                log::trace!("[SSH] file not found: {}", remote_path.display());
                return Ok(false);
            }
            Err(openssh_sftp_client::Error::SftpError(SftpErrorKind::NoSuchFile, _)) => {
                log::trace!("[SSH] file not found: {}", remote_path.display());
                return Ok(false);
            }
            Err(e) => return Err(e.into()),
        };
        let Some(file_type) = metadata.file_type() else {
            return Err(Error::CouldntGetRemoteFileType);
        };
        if !file_type.is_file() {
            return Err(Error::PathExistsButNotFile(format!("{}", remote_path.display())));
        }
        log::trace!("[SSH] file exists: {}", remote_path.display());
        Ok(true)
    }

    pub(crate) async fn process_exists(&self, process_name: &str) -> Result<bool, Error> {
        log::trace!("[SSH] checking process exists...: {}", process_name);
        // example for showing executing command and parsing output

        let mut ps_process = self.session.raw_command(vec!["ps", "auwx"].into_iter().map(|s| escape(s.into()).to_string()).collect::<Vec<_>>().join(" "))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .await?;

        let stdout = ps_process.stdout().take().ok_or(Error::CouldntTakeRemoteProcessStdout)?;
        let mut line_stream = BufReader::new(stdout).lines();

        let first_line = line_stream.next_line().await?;
        let Some(first_line) = first_line else {
            return Err(Error::PsCommandNoLineOutput);
        };
        let headers = first_line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(headers.len(), 11);
        assert_eq!(headers.last().expect("last"), &"COMMAND");

        let regex = Regex::new(r"^(?:[^\s]+\s+){10}(.*)$").expect("hardcoded regex");
        let found = loop {
            let Some(record) = line_stream.next_line().await? else {
                log::trace!("[SSH] process not found: {}", process_name);
                break false;
            };
            let captures = regex.captures(&record).ok_or(Error::ParseFailedPsOutputLine)?;
            let command = captures.get(1).expect("should have capture").as_str();
            if command.contains(process_name) {
                log::trace!("[SSH] process found: {}", process_name);
                break true;
            };
        };

        ps_process.wait().await?;

        Ok(found)
    }
}

async fn wait_for_ssh_connectable(ip: Ipv4Addr, port: u16) -> Result<(), Error> {
    
    // lightweight ssh connection check than connect

    let mut interval = interval(Duration::from_secs(10));

    loop {
        match timeout(Duration::from_secs(5), TcpStream::connect((ip, port))).await {
            Err(_) => {
                log::trace!("waiting for ssh: timeout");
                interval.tick().await;
            }
            Ok(Err(e)) => {
                log::trace!("waiting for ssh: {}", e);
                interval.tick().await;
            }
            Ok(Ok(_)) => {
                return Ok(());
            }
        }
    }
}



