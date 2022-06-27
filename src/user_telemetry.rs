use collection::config::CollectionConfig;
use collection::operations::types::{CollectionStatus, OptimizersStatus};
use collection::telemetry::{CollectionTelemetryMessage, CollectionTelemetrySender};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use uuid::Uuid;

use crate::settings::Settings;

pub struct CollectionTelemetryCollector {
    config: CollectionConfig,
    init_time: std::time::Duration,
    status: CollectionStatus,
    optimizer_status: OptimizersStatus,
    vectors_count: usize,
    segments_count: usize,
    disk_data_size: usize,
    ram_data_size: usize,
}

pub struct UserTelemetryCollector {
    process_id: Uuid,
    settings: Option<Settings>,
    collections: HashMap<String, CollectionTelemetryCollector>,
    collection_receiver: Receiver<CollectionTelemetryMessage>,
    collection_sender: CollectionTelemetrySender,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryApp {
    version: String,
    debug: bool,
    web_feature: bool,
    service_debug_feature: bool,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetrySystem {
    distribution: Option<String>,
    distribution_version: Option<String>,
    is_docker: bool,
    // TODO(ivan) parse dockerenv file
    // docker_version: Option<String>,
    cores: Option<usize>,
    ram_size: Option<usize>,
    disk_size: Option<usize>,
    cpu_flags: String,
    // TODO(ivan) get locale and region
    // locale: Option<String>,
    // region: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryServiceConfig {
    grpc_enable: bool,
    max_request_size_mb: usize,
    max_workers: Option<usize>,
    enable_cors: bool,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryP2pConfig {
    connection_pool_size: usize,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryConsensusConfig {
    max_message_queue_size: usize,
    tick_period_ms: u64,
    bootstrap_timeout_sec: u64,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryClusterConfig {
    enabled: bool,
    grpc_timeout_ms: u64,
    p2p: UserTelemetryP2pConfig,
    consensus: UserTelemetryConsensusConfig,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryConfigs {
    service_config: UserTelemetryServiceConfig,
    cluster_config: UserTelemetryClusterConfig,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryCollection {
    id: String,
    config: CollectionConfig,
    creation_time: std::time::Duration,
    status: CollectionStatus,
    optimizer_status: OptimizersStatus,
    vectors_count: usize,
    segments_count: usize,
    disk_data_size: usize,
    ram_data_size: usize,
}

#[derive(Serialize, Clone)]
pub struct UserTelemetryData {
    id: String,
    app: UserTelemetryApp,
    system: UserTelemetrySystem,
    configs: UserTelemetryConfigs,
    collections: Vec<UserTelemetryCollection>,
}

impl UserTelemetryCollector {
    pub fn new() -> Self {
        let (collection_sender, collection_receiver) = channel();
        Self {
            process_id: Uuid::new_v4(),
            settings: None,
            collections: HashMap::new(),
            collection_receiver,
            collection_sender: Arc::new(parking_lot::Mutex::new(Some(collection_sender))),
        }
    }

    pub fn put_settings(&mut self, settings: Settings) {
        self.settings = Some(settings);
    }

    pub fn get_collection_sender(&self) -> CollectionTelemetrySender {
        self.collection_sender.clone()
    }

    #[allow(dead_code)]
    pub fn prepare_data(&mut self) -> UserTelemetryData {
        self.process_messages();
        UserTelemetryData {
            id: self.process_id.to_string(),
            app: self.get_app_data(),
            system: self.get_system_data(),
            configs: self.get_configs_data(),
            collections: self.get_collections_data(),
        }
    }

    fn process_messages(&mut self) {
        while let Ok(message) = self.collection_receiver.try_recv() {
            match message {
                CollectionTelemetryMessage::NewSegment {
                    id,
                    config,
                    creation_time,
                } => {
                    let collection = CollectionTelemetryCollector::new(config, creation_time);
                    self.collections.insert(id, collection);
                }
                CollectionTelemetryMessage::LoadSegment {
                    id,
                    config,
                    load_time,
                } => {
                    let collection = CollectionTelemetryCollector::new(config, load_time);
                    self.collections.insert(id, collection);
                }
                CollectionTelemetryMessage::Info {
                    id,
                    status,
                    optimizer_status,
                    vectors_count,
                    segments_count,
                    disk_data_size,
                    ram_data_size,
                } => {
                    if let Some(collection) = self.collections.get_mut(&id) {
                        collection.status = status;
                        collection.optimizer_status = optimizer_status;
                        collection.vectors_count = vectors_count;
                        collection.segments_count = segments_count;
                        collection.disk_data_size = disk_data_size;
                        collection.ram_data_size = ram_data_size;
                    }
                }
            }
        }
    }

    fn get_app_data(&self) -> UserTelemetryApp {
        UserTelemetryApp {
            version: env!("CARGO_PKG_VERSION").to_string(),
            debug: cfg!(debug_assertions),
            web_feature: cfg!(feature = "web"),
            service_debug_feature: cfg!(feature = "service_debug"),
        }
    }

    fn get_system_data(&self) -> UserTelemetrySystem {
        let distribution = if let Ok(release) = sys_info::linux_os_release() {
            release.id
        } else {
            sys_info::os_type().ok()
        };
        let distribution_version = if let Ok(release) = sys_info::linux_os_release() {
            release.version_id
        } else {
            sys_info::os_release().ok()
        };
        let mut cpu_flags = String::new();
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if std::arch::is_x86_feature_detected!("sse") {
                cpu_flags += "sse,";
            }
            if std::arch::is_x86_feature_detected!("avx") {
                cpu_flags += "avx,";
            }
            if std::arch::is_x86_feature_detected!("avx2") {
                cpu_flags += "avx2,";
            }
            if std::arch::is_x86_feature_detected!("fma") {
                cpu_flags += "fma,";
            }
            if std::arch::is_x86_feature_detected!("avx512f") {
                cpu_flags += "avx512f,";
            }
        }
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            if std::arch::is_aarch64_feature_detected!("neon") {
                cpu_flags += "neon,";
            }
        }
        UserTelemetrySystem {
            distribution,
            distribution_version,
            is_docker: cfg!(unix) && Path::new("/.dockerenv").exists(),
            cores: sys_info::cpu_num().ok().map(|x| x as usize),
            ram_size: sys_info::mem_info().ok().map(|x| x.total as usize),
            disk_size: sys_info::disk_info().ok().map(|x| x.total as usize),
            cpu_flags,
        }
    }

    fn get_configs_data(&self) -> UserTelemetryConfigs {
        let settings = self
            .settings
            .clone()
            .expect("User settings have been not provided");
        UserTelemetryConfigs {
            service_config: UserTelemetryServiceConfig {
                grpc_enable: settings.service.grpc_port.is_some(),
                max_request_size_mb: settings.service.max_request_size_mb,
                max_workers: settings.service.max_workers,
                enable_cors: settings.service.enable_cors,
            },
            cluster_config: UserTelemetryClusterConfig {
                enabled: settings.cluster.enabled,
                grpc_timeout_ms: settings.cluster.grpc_timeout_ms,
                p2p: UserTelemetryP2pConfig {
                    connection_pool_size: settings.cluster.p2p.connection_pool_size,
                },
                consensus: UserTelemetryConsensusConfig {
                    max_message_queue_size: settings.cluster.consensus.max_message_queue_size,
                    tick_period_ms: settings.cluster.consensus.tick_period_ms,
                    bootstrap_timeout_sec: settings.cluster.consensus.bootstrap_timeout_sec,
                },
            },
        }
    }

    fn get_collections_data(&self) -> Vec<UserTelemetryCollection> {
        let mut result = Vec::new();
        for (id, collection) in &self.collections {
            result.push(UserTelemetryCollection {
                id: id.clone(),
                config: collection.config.clone(),
                creation_time: collection.init_time,
                status: collection.status,
                optimizer_status: collection.optimizer_status.clone(),
                vectors_count: collection.vectors_count,
                segments_count: collection.segments_count,
                disk_data_size: collection.disk_data_size,
                ram_data_size: collection.ram_data_size,
            });
        }
        result
    }
}

impl CollectionTelemetryCollector {
    pub fn new(config: CollectionConfig, init_time: std::time::Duration) -> Self {
        Self {
            config,
            init_time,
            status: CollectionStatus::Green,
            optimizer_status: OptimizersStatus::Ok,
            vectors_count: 0,
            segments_count: 0,
            disk_data_size: 0,
            ram_data_size: 0,
        }
    }
}
