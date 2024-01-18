// Copyright 2022 - 2024 Wenmeng See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// Author: tickbh
// -----
// Created Date: 2024/01/16 10:59:37

// use std::net::SocketAddr;

use std::process::id;
use std::thread::sleep;
use std::time::Duration;
use std::{
    fmt::Display,
    fs::File,
    io::{self, Read, Write},
    net::{AddrParseError, IpAddr, Ipv4Addr, SocketAddr},
    os::windows::process,
    path::PathBuf,
    process::exit,
    str::FromStr,
};

use bpaf::*;
use log::{Level, LevelFilter};
use tokio::task::spawn_blocking;
use webparse::{Url, Request};
use wenmeng::Client;

use crate::{
    option::proxy_config,
    reverse::{HttpConfig, LocationConfig, ServerConfig, UpstreamConfig},
    ConfigHeader, ConfigLog, ConfigOption, FileServer, HeaderOper, ProxyConfig, ProxyResult,
};

#[derive(Debug, Clone, Copy)]
pub struct WrapAddr(pub SocketAddr);

impl FromStr for WrapAddr {
    type Err = AddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with(":") {
            let addr = format!("127.0.0.1{s}").parse::<SocketAddr>()?;
            Ok(WrapAddr(addr))
        } else {
            let addr = s.parse::<SocketAddr>()?;
            Ok(WrapAddr(addr))
        }
    }
}

impl Display for WrapAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}

#[derive(Debug, Clone, Bpaf)]
#[allow(dead_code)]
struct Shared {
    /// 输入控制台的监听地址
    #[bpaf(
        fallback(WrapAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8837))),
        display_fallback
    )]
    pub(crate) control: WrapAddr,
    /// 禁用默认输出
    pub(crate) disable_stdout: bool,
    /// 禁用控制微端
    pub(crate) disable_control: bool,
    /// 是否显示更多日志
    #[bpaf(short, long)]
    pub(crate) verbose: bool,
    /// 设置默认等级
    pub(crate) default_level: Option<LevelFilter>,
}

#[derive(Debug, Clone, Bpaf)]
#[allow(dead_code)]
struct RunConfig {
    /// 配置文件路径
    #[bpaf(short, long)]
    pub(crate) config: String,

    /// 写入进程id文件
    #[bpaf(long)]
    pub(crate) pidfile: Option<String>
}


#[derive(Debug, Clone, Bpaf)]
#[allow(dead_code)]
struct StopConfig {
    /// 配置文件路径
    #[bpaf(short, long)]
    pub(crate) config: Option<String>,

    /// 写入进程id文件
    #[bpaf(long)]
    pub(crate) pidfile: Option<String>,

    /// 控制微端地址
    #[bpaf(long)]
    pub(crate) url: Option<String>,
}


#[derive(Debug, Clone, Bpaf)]
#[allow(dead_code)]
struct FileServerConfig {
    /// 静态文件根目录路径
    #[bpaf(short, long, fallback(String::new()))]
    pub(crate) root: String,
    #[bpaf(
        short,
        long,
        fallback(WrapAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80))),
        display_fallback
    )]
    /// 监听地址
    pub(crate) listen: WrapAddr,
    /// 域名地址
    #[bpaf(short, long)]
    pub(crate) domain: Option<String>,
    /// 是否支持目录
    #[bpaf(short, long)]
    pub(crate) browse: bool,
    /// 访问日志放的位置如"logs/access.log trace"
    #[bpaf(long)]
    pub(crate) access_log: Option<String>,
}

#[derive(Debug, Clone, Bpaf)]
#[allow(dead_code)]
struct ReverseProxyConfig {
    /// 负载均衡来源地址
    #[bpaf(
        short,
        long,
        fallback(WrapAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80))),
        display_fallback
    )]
    pub(crate) from: WrapAddr,
    /// 负载均衡映射地址
    #[bpaf(short, long)]
    pub(crate) to: WrapAddr,
    /// 头部信息修改如 "proxy x-forward-for {client_ip}"
    #[bpaf(short('H'), long)]
    pub(crate) header: Vec<ConfigHeader>,
    /// 访问日志放的位置如"logs/access.log trace"
    #[bpaf(long)]
    pub(crate) access_log: Option<String>,
    /// 是否映射到https上
    #[bpaf(long)]
    pub(crate) tls: bool,
}

#[derive(Debug, Clone, Bpaf)]
#[allow(dead_code)]
struct VersionConfig {}

#[derive(Debug, Clone)]
enum Command {
    Proxy(ProxyConfig),
    Run(RunConfig),
    Start(RunConfig),
    Stop(StopConfig),
    FileServer(FileServerConfig),
    ReverseProxy(ReverseProxyConfig),
    Version(VersionConfig),
}

fn parse_command() -> impl Parser<(Command, Shared)> {
    let action = proxy_config().map(Command::Proxy);
    let action = construct!(action, shared()).to_options().command("proxy");

    let run = run_config().map(Command::Run);
    let run = construct!(run, shared()).to_options().command("run");

    let start = run_config().map(Command::Start);
    let start = construct!(start, shared()).to_options().command("start");
    
    let stop = stop_config().map(Command::Stop);
    let stop = construct!(stop, shared()).to_options().command("start");


    let file_config = file_server_config().map(Command::FileServer);
    let file_config = construct!(file_config, shared())
        .to_options()
        .command("file-server");

    let reverse_config = reverse_proxy_config().map(Command::ReverseProxy);
    let reverse_config = construct!(reverse_config, shared())
        .to_options()
        .command("reverse-proxy");

    let version_config = version_config().map(Command::Version);
    let version_config = construct!(version_config, shared())
        .to_options()
        .command("version");
    construct!([action, run, start, stop, file_config, reverse_config, version_config])
}


fn read_config_from_path(path: &String) -> ProxyResult<ConfigOption> {
    let path = PathBuf::from(path);
    let mut file = File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let extension = path.extension().unwrap().to_string_lossy().to_string();
    let option = match &*extension {
        "yaml" => serde_yaml::from_str::<ConfigOption>(&contents).map_err(|e| {
            println!("parse error msg = {:?}", e);
            io::Error::new(io::ErrorKind::Other, "parse yaml error")
        })?,
        "toml" => toml::from_str::<ConfigOption>(&contents).map_err(|e| {
            println!("parse error msg = {:?}", e);
            io::Error::new(io::ErrorKind::Other, "parse toml error")
        })?,
        _ => {
            let e = io::Error::new(io::ErrorKind::Other, "unknow format error");
            return Err(e.into());
        }
    };
    Ok(option)
}

fn kill_process_by_id(id: String) -> Option<i32> {
    if id == String::new() {
        return Some(-1);
    }
    let child = if cfg!(target_os = "windows") {
        ::std::process::Command::new("kill")
                .arg(id)
                .output()
                .expect("failed to execute process")
    } else {
        ::std::process::Command::new("kill")
                .args(["-TERM".to_string(), id.clone()])
                .output()
                .expect("failed to execute process")
    };
    return child.status.code();
}

pub async fn parse_env() -> ProxyResult<ConfigOption> {
    let (command, shared) = parse_command().run();
    let mut option = ConfigOption::default();
    option.default_level = shared.default_level;
    option.disable_control = shared.disable_control;
    option.disable_stdout = shared.disable_stdout;
    option.control = shared.control.0;
    if shared.verbose {
        option.default_level = Some(LevelFilter::Trace);
    }
    match command {
        Command::Proxy(proxy) => {
            option.proxy = Some(proxy);
            option.after_load_option()?;
            return Ok(option);
        }
        Command::Run(config) => {
            let mut option = read_config_from_path(&config.config)?;
            if shared.verbose {
                option.default_level = Some(LevelFilter::Trace);
            }
            option.after_load_option()?;
            if let Some(pid) = config.pidfile {
                let mut file = File::create(pid)?;
                file.write_all(&format!("{}", id()).as_bytes())?;
            }
            return Ok(option);
        }
        Command::Start(_) => {
            let args = std::env::args().collect::<Vec<String>>();
            let mut command = std::process::Command::new(&args[0]);
            command.arg("run");
            for value in &args[2..] {
                command.arg(&*value);
            }
            command.spawn().expect("failed to start wmproxy");
            exit(0);
        }
        Command::Stop(config) => {
            if let Some(pid) = config.pidfile {
                let mut file = File::open(pid)?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                exit(kill_process_by_id(content).unwrap_or(0));
            }
            let url = if let Some(config) = config.config {
                let option = read_config_from_path(&config)?;
                format!("http://{}", option.control)
            } else if let Some(url) = config.url {
                url
            } else {
                println!("必须传入参数pidfile或者config或者url之一");
                exit(0);
            };

            let mut url = Url::parse(url.into_bytes())?;
            url.path = "/stop".to_string();

            let req = Request::builder().method("GET").url(url.clone()).body("")?;
            println!("url = {:?}", req.get_connect_url());
            let client = Client::builder().
                url(url)?
                .connect().await?;
            
            let (mut recv, _sender) = client.send2(req.into_type()).await?;
            let res = recv.recv().await.unwrap()?;
            if res.status() == 200 {
                println!("关闭成功!");
            } else {
                println!("微端响应:{}!", res.status());
            }
            exit(0);
        }
        Command::FileServer(file) => {
            let mut http = HttpConfig::new();
            let mut server = ServerConfig::new(file.listen.0);
            let mut location = LocationConfig::new();
            location.file_server = Some(FileServer::new(file.root, "".to_string()));
            if let Some(access) = file.access_log {
                http.comm.access_log = Some(ConfigLog::new(
                    "access".to_string(),
                    "main".to_string(),
                    Level::Trace,
                ));
                http.comm.log_names.insert("access".to_string(), access);
            }
            server.location.push(location);
            http.server.push(server);
            option.http = Some(http);
            option.after_load_option()?;
            return Ok(option);
        }
        Command::ReverseProxy(reverse) => {
            let mut http = HttpConfig::new();
            let mut server = ServerConfig::new(reverse.from.0);
            let mut location = LocationConfig::new();
            let up_name = "server".to_string();
            let upstream = UpstreamConfig::new_single(up_name.clone(), reverse.to.0);
            let url = if reverse.tls {
                let name = format!("https://{}", up_name);
                Url::parse(name.into_bytes())?
            } else {
                let name = format!("http://{}", up_name);
                Url::parse(name.into_bytes())?
            };
            location.comm.proxy_url = Some(url);
            location.headers = reverse.header;
            http.upstream.push(upstream);
            if let Some(access) = reverse.access_log {
                http.comm.access_log = Some(ConfigLog::new(
                    "access".to_string(),
                    "main".to_string(),
                    Level::Trace,
                ));
                http.comm.log_names.insert("access".to_string(), access);
            }
            server.location.push(location);
            http.server.push(server);
            option.http = Some(http);
            option.after_load_option()?;
            return Ok(option);
        }
        Command::Version(_) => {
            println!("version = {}", "0.2.4");
            exit(0);
        }
    }
}