// src/manager.rs

#[cfg(windows)]
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use anyhow::{Context, Result, anyhow};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::process::{Child, Command};

use crate::service::{ServiceConfig, ServicesFile, build_args, exec_file_name};

/// Snashot of service status
/// To porcessing list of services
#[derive(Debug, Clone)]
pub struct ServiceStatusSnapshot {
    pub config: ServiceConfig,
    pub running: bool,
    pub pid: Option<u32>,
}
/// Structure of services
/// Include config, process and pid
pub struct ManagedService {
    pub config: ServiceConfig,
    pub process: Option<Child>,
    pub last_known_pid: Option<u32>,    // to catch pid who not started by app manager  
}
impl ManagedService {
    fn new(config: ServiceConfig) -> Self {
        Self {
            config,
            process: None,
            last_known_pid: None,
        }
    }
}
/// Structuer of app manager
/// Include services, order, process related and config path
/// Global parameter: listen address and keep alive interval
pub struct ServiceManager {
    pub services: HashMap<String, ManagedService>,
    pub service_order: Vec<String>,
    sys: System,
    config_path: String,
    pub config_listen: Option<String>,
    pub keep_alive_interval: u64,
}
impl ServiceManager {
    pub fn new(config_file: &str) -> Result<Self> {
        // Read and parse YAML config file
        let content = std::fs::read_to_string(config_file)
            .context("Failed to read config file")?;
        let service_file: ServicesFile = serde_yaml::from_str(&content)
            .context("Failed to parse YAML")?;
        // Storage services and their order
        let mut services = HashMap::new();
        let mut service_order = Vec::new();
        // Help to deduplicate of service order
        let mut seen_ids = HashSet::new();
        // Detect processes and pids
        // use System::new() to save memory usage instead of System::new_all()
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        // Read services from config
        for cfg in service_file.services {
            let id = cfg.id.clone();
            // Avoid duplication service in order (which is not hash)
            if seen_ids.contains(&id) {
                eprintln!("⚠️ Warning: Duplicate service ID '{}' found in config. Skipping duplicate.", id);
                continue;
            }
            // Push service into order to show
            seen_ids.insert(id.clone());
            service_order.push(cfg.id.clone());

            let mut svc = ManagedService::new(cfg);

            let exec_name = exec_file_name(&svc.config.exec);
            // Find if process is already existing
            let found_proc = sys.processes().values().find(|p| {
                let _p_name = p.name();
                p.name().eq_ignore_ascii_case(exec_name)
            });
            // If existing, get PIDs
            if let Some(proc) = found_proc {
                let pid = proc.pid().as_u32();
                println!(
                    "🔗 Adopted existing service: {} (PID: {})",
                    svc.config.name, pid
                );
                svc.last_known_pid = Some(pid); // Catch pid who not started by app manager
            }
            services.insert(svc.config.id.clone(), svc);
        }
        Ok(Self {
            services,
            service_order,
            sys,
            config_path: config_file.to_string(),
            config_listen: service_file.listen,
            keep_alive_interval: service_file.keep_alive.unwrap_or(0),
        })
    }
    // Check if serivce is already running
    pub fn is_running(&mut self, id: &str) -> bool {
        // Check by ID
        if let Some(svc) = self.services.get_mut(id) {
            if let Some(child) = &mut svc.process {
                match child.try_wait() {
                    Ok(None) => return true,
                    Ok(Some(_)) | Err(_) => {
                        svc.process = None;
                    }
                }
            }
        }
        // Check already running service by processes PIDs 
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        let (last_pid, exec_name) = match self.services.get(id) {
            Some(s) => (s.last_known_pid, s.config.exec.clone()),
            None => return false,
        };

        if let Some(pid) = last_pid {
            if self.sys.process(Pid::from_u32(pid)).is_some() {
                return true;
            }
        }
        // Check already running service by processes names
        let target = exec_file_name(&exec_name);
        self.sys.processes().values().any(|p| {
            let n = p.name();
            n.eq_ignore_ascii_case(target) || n.eq_ignore_ascii_case(&format!("{}.exe", target))
        })
    }
    /// Start
    pub async fn start(&mut self, id: &str) -> Result<()> {
        // Check if already running
        if self.is_running(id) {
            println!("Service {} is already running.", id);
            return Ok(());
        }

        let svc = self
            .services
            .get_mut(id)
            .ok_or_else(|| anyhow!("Service id not found"))?;
        // Combine command args
        let args = build_args(&svc.config.args, &svc.config.env);
        // Combine binary path
        let exec_path = if let Some(dir) = &svc.config.working_dir {
            Path::new(dir).join(&svc.config.exec)
        } else {
            Path::new(&svc.config.exec).to_path_buf()
        };
        // Combine command
        let mut cmd = Command::new(&exec_path);
        cmd.args(args);

        if let Some(dir) = &svc.config.working_dir {
            cmd.current_dir(dir);
        }
        // For windows to process creation flags
        // Add extra flags 0x00000008 to avoid blocking
        #[cfg(windows)]
        {
            let flags = svc
                .config
                .windows
                .as_ref()
                .and_then(|w| w.creation_flags)
                .unwrap_or(0x00000008);
            cmd.creation_flags(flags);
        }
        // Avoid blocking by main process
        cmd.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
        // Run command
        let child = cmd
            .spawn()
            .context(format!("Failed to spawn {}", svc.config.name))?;
        let pid = child.id().unwrap_or(0);
        // record process and its pid
        svc.process = Some(child);
        svc.last_known_pid = Some(pid);

        println!("Started service \"{}\" (PID: {})", id, pid);
        Ok(())
    }
    /// Stop
    pub async fn stop(&mut self, id: &str) -> Result<()> {
        // Stop process
        let svc = self
            .services
            .get_mut(id)
            .ok_or_else(|| anyhow!("Service id not found"))?;

        // Get the parent process PID
        // Use last_known_pid, it is same as process handle id
        let target_pid_u32 = svc.last_known_pid.or_else(|| {
            svc.process.as_ref().map(|p| p.id().unwrap_or(0))
        });
        // Try to clear the process tree (some apps has more than one process)
        if let Some(pid_val) = target_pid_u32 {
            if pid_val > 0 {
                self.sys.refresh_processes(ProcessesToUpdate::All, true);
                let parent_pid = Pid::from_u32(pid_val);

                // Find all child process of parent process
                let children: Vec<Pid> = self.sys.processes()
                    .iter().
                    filter(|(_, p)| p.parent() == Some(parent_pid))
                    .map(|(pid, _)| * pid)
                    .collect();

                // Kill child process first (e.g. Worker)
                for child_pid in children {
                    if let Some(proc) = self.sys.process(child_pid) {
                        if proc.kill() {
                            println!("Killed child process {}: {}", id, child_pid);
                        }
                    }
                }
            }
        }
        // Kill main process handle (e.g. Monitor)
        if let Some(mut child) = svc.process.take() {
            // Try to kill process
            let _ = child.kill().await;
            let _ = child.wait().await;
            println!("Stopped service \"{}\" via handle", id);
        } else if let Some(pid_val) = target_pid_u32 {
            // If lose handle (e.g. restart apps), try to use sysinfo to kill main process
            if let Some(proc) = self.sys.process(Pid::from_u32(pid_val)) {
                proc.kill();
                println!("Killed orphaned main process of {}: {}", id, pid_val);
            }
        }
        // Kill by process name
        // If still survival under PID killer, use process name to kill
        // Only use when process is running to prevent kill wrong one
        let target_exec = svc.config.exec.clone();
        let target_name = exec_file_name(&target_exec);

        self.sys.refresh_processes(ProcessesToUpdate::All, true);

        // Only when escape from PID killer
        let remining_pids: Vec<Pid> = self.sys.processes().values()
            .filter(|p| p.name().eq_ignore_ascii_case(target_name))
            .map(|p| p.pid())
            .collect();

        if !remining_pids.is_empty() {
            println!("⚠️ Warning: Found lingering processes for {}, cleaning up by name...", id);
            for pid in remining_pids {
                if let Some(proc) = self.sys.process(pid) {
                    proc.kill();
                    println!("Killed lingering process {} (PID: {})", target_name, pid);
                }
            }
        }

        // clear PID state
        svc.last_known_pid = None;


        Ok(())
    }
    /// Restart
    pub async fn restart(&mut self, id: &str) -> Result<()> {
        self.stop(id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        self.start(id).await
    }
    /// List
    pub fn list(&mut self) -> Vec<ServiceStatusSnapshot> {
        let mut results = Vec::new();

        let order = self.service_order.clone();

        for id in order {
            if self.services.contains_key(&id) {
                
                let running = self.is_running(&id);
                
                if let Some(svc) = self.services.get(&id) {
                     results.push(ServiceStatusSnapshot {
                        config: svc.config.clone(),
                        running,
                        pid: svc.last_known_pid,
                    });
                }
            }
        }
        results
    }

    pub fn save_to_disk(&self) -> Result<()> {
        let mut configs = Vec::new();
        let mut saved_ids = HashSet::new();

        for id in &self.service_order {
            if saved_ids.contains(id) { continue; }

            if let Some(svc) = self.services.get(id) {
                configs.push(svc.config.clone());
                saved_ids.insert(id.clone());
            }
        }
        let wrapper = ServicesFile {
            services: configs,
            listen: self.config_listen.clone(),
            keep_alive: if self.keep_alive_interval > 0 { Some(self.keep_alive_interval) } else { None },
        };

        let yaml = serde_yaml::to_string(&wrapper)?;

        std::fs::write(&self.config_path, yaml)?;
        Ok(())
    }

    pub fn upsert_service(&mut self, config: ServiceConfig) -> Result<()> {
        let id = config.id.clone();
        if !self.service_order.contains(&id) {
            self.service_order.push(id.clone());
        }

        if let Some(svc) = self.services.get_mut(&config.id) {
            svc.config = config;
        } else {
            self.services
                .insert(config.id.clone(), ManagedService::new(config));
        }
        self.save_to_disk()
    }

    pub async fn remove_service(&mut self, id: &str) -> Result<()> {
        let _ = self.stop(id).await;

        if self.services.remove(id).is_some() {
            self.service_order.retain(|x| x != id);
            self.save_to_disk()?;
            Ok(())
        } else {
            Err(anyhow!("Service not found"))
        }
    }

    pub fn reorder_services(&mut self, new_order: Vec<String>) -> Result<()> {

        let mut unique_order = Vec::new();
        let mut seen = HashSet::new();
        
        for id in new_order {

            if self.services.contains_key(&id) && !seen.contains(&id) {
                unique_order.push(id.clone());
                seen.insert(id);
            }
        }
        

        for id in self.services.keys() {
            if !seen.contains(id) {
                unique_order.push(id.clone());
            }
        }

        self.service_order = unique_order;
        self.save_to_disk()
    }

    pub fn set_global_config(&mut self, keep_alive: u64) -> Result<()> {
        self.keep_alive_interval = keep_alive;
        self.save_to_disk()
    }
}
