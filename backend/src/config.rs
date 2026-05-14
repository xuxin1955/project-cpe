/*
 * @Author: 1orz cloudorzi@gmail.com
 * @Date: 2025-12-09 17:34:01
 * @LastEditors: 1orz cloudorzi@gmail.com
 * @LastEditTime: 2025-12-13 12:45:58
 * @FilePath: /udx710-backend/backend/src/config.rs
 * @Description: 
 * 
 * Copyright (c) 2025 by 1orz, All Rights Reserved. 
 */

//! 配置管理模块
//!
//! 使用 JSON 文件存储用户配置，支持热更新

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

/// Webhook 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub url: String,
    pub forward_sms: bool,
    pub forward_calls: bool,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub secret: String,  // 可选的签名密钥
    #[serde(default = "default_sms_template")]
    pub sms_template: String,  // 短信 payload 模板
    #[serde(default = "default_call_template")]
    pub call_template: String,  // 通话 payload 模板
}

/// 默认短信模板 (PushPlus)
fn default_sms_template() -> String {
    r#"{
  "title": "新短信",
  "content": "发件人: {{phone_number}}\n内容: {{content}}",
  "time": "{{timestamp}}"
}"#.to_string()
}

/// 默认通话模板 (PushPlus)
fn default_call_template() -> String {
    r#"{
  "title": "来电通知",
  "content": "号码: {{phone_number}}\n类型: {{direction_cn}}\n时间: {{start_time}}\n时长: {{duration}} 秒\n已接听: {{answered}}"
}"#.to_string()
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            forward_sms: true,
            forward_calls: true,
            headers: HashMap::new(),
            secret: String::new(),
            sms_template: default_sms_template(),
            call_template: default_call_template(),
        }
    }
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub webhook: WebhookConfig,
    // 未来可以添加更多配置项
}

/// 配置管理器
pub struct ConfigManager {
    config: Arc<RwLock<AppConfig>>,
    config_path: PathBuf,
}

impl ConfigManager {
    /// 创建新的配置管理器
    pub fn new(config_path: PathBuf) -> Self {
        let config = if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => {
                    match serde_json::from_str::<AppConfig>(&content) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            warn!(error = %e, "Failed to parse config file, using defaults");
                            AppConfig::default()
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to read config file, using defaults");
                    AppConfig::default()
                }
            }
        } else {
            info!("No config file found, using defaults");
            AppConfig::default()
        };

        let manager = Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        };
        
        // 保存默认配置（如果文件不存在）
        if !manager.config_path.exists() {
            let _ = manager.save();
        }
        
        manager
    }
    
    /// 获取当前配置
    #[allow(dead_code)]
    pub fn get(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }
    
    /// 获取 Webhook 配置
    pub fn get_webhook(&self) -> WebhookConfig {
        self.config.read().unwrap().webhook.clone()
    }
    
    /// 更新 Webhook 配置
    pub fn set_webhook(&self, webhook: WebhookConfig) -> Result<(), String> {
        {
            let mut config = self.config.write().unwrap();
            config.webhook = webhook;
        }
        self.save()
    }
    
    /// 更新整个配置
    #[allow(dead_code)]
    pub fn set(&self, config: AppConfig) -> Result<(), String> {
        {
            let mut current = self.config.write().unwrap();
            *current = config;
        }
        self.save()
    }
    
    /// 保存配置到文件
    pub fn save(&self) -> Result<(), String> {
        let config = self.config.read().unwrap();
        let content = serde_json::to_string_pretty(&*config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        // 确保目录存在
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        
        fs::write(&self.config_path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
        
        Ok(())
    }
    
    /// 重新加载配置
    #[allow(dead_code)]
    pub fn reload(&self) -> Result<(), String> {
        if !self.config_path.exists() {
            return Err("Config file does not exist".to_string());
        }
        
        let content = fs::read_to_string(&self.config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        
        let new_config: AppConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;
        
        {
            let mut config = self.config.write().unwrap();
            *config = new_config;
        }
        
        Ok(())
    }
}

/// 获取默认配置文件路径
pub fn get_default_config_path() -> PathBuf {
    // 尝试 /data/config.json（设备上的持久化目录）
    let device_path = PathBuf::from("/data/config.json");
    if device_path.parent().map(|p| p.exists()).unwrap_or(false) {
        return device_path;
    }
    
    // 回退到当前目录
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("config.json")
}
