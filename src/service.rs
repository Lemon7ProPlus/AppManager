/// src/service.rs
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

/// Service config files structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub id: String,
    pub name :String,
    pub exec: String,
    pub working_dir: Option<String>,
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
    pub windows: Option<WindowsOptions>,
    pub autorun: Option<bool>,
    pub url: Option<String>,
}

/// Windows start options
/// 0x08000000: hide
/// 0x00000010: show
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowsOptions {
    pub creation_flags: Option<u32>,
}

/// Full config structure
/// Includes keep_alive interval and listen address
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServicesFile {
    pub listen: Option<String>,
    pub keep_alive: Option<u64>,
    pub services: Vec<ServiceConfig>,
}

/// Combine the args of command
pub fn build_args(args: &[String], env: &Option<HashMap<String, String>>) -> Vec<String> {
    args.iter().map(|arg| {
        let mut s = arg.clone();
        if let Some(envkv) = env {
            for (k, v) in envkv {
                s = s.replace(&format!("{{{}}}", k), v);
            }
        }
        s
    }).collect()
}

/// Get the file name of exec
pub fn exec_file_name(exec_path: &str) -> &str {
    Path::new(exec_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(exec_path)
}