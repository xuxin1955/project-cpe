use crate::config::{ConfigManager, WebhookConfig};
use crate::db::{CallRecord, SmsMessage};
use chrono::{DateTime, FixedOffset, Utc};
use reqwest::Client;
use std::sync::Arc;

fn to_beijing_time(iso: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso) {
        let bj = dt.with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap());
        return bj.format("%Y-%m-%d %H:%M:%S").to_string();
    }
    if chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%d %H:%M:%S").is_ok() {
        return iso.to_string();
    }
    iso.to_string()
}

pub struct WebhookSender {
    client: Client,
    config_manager: Arc<ConfigManager>,
}

impl WebhookSender {
    pub fn new(config_manager: Arc<ConfigManager>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            config_manager,
        }
    }

    fn get_config(&self) -> WebhookConfig {
        self.config_manager.get_webhook()
    }

    pub async fn forward_sms(&self, message: &SmsMessage) -> Result<(), String> {
        let config = self.get_config();
        if !config.enabled || !config.forward_sms || config.url.is_empty() {
            return Ok(());
        }
        let payload = render_sms_template(&config.sms_template, message);
        self.send_webhook_raw(&config, &payload).await
    }

    pub async fn forward_call(&self, call: &CallRecord) -> Result<(), String> {
        let config = self.get_config();
        if !config.enabled || !config.forward_calls || config.url.is_empty() {
            return Ok(());
        }
        let payload = render_call_template(&config.call_template, call);
        self.send_webhook_raw(&config, &payload).await
    }

    async fn send_webhook_raw(
        &self,
        config: &WebhookConfig,
        payload: &str,
    ) -> Result<(), String> {
        let mut request = self.client.post(&config.url);
        for (key, value) in &config.headers {
            request = request.header(key, value);
        }
        request = request.header("Content-Type", "application/json");
        if !config.secret.is_empty() {
            let signature = compute_signature(&config.secret, payload);
            request = request.header("X-Webhook-Signature", signature);
        }
        let response = request
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| format!("Failed to send webhook: {}", e))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("Webhook returned error status {}: {}", status, body))
        }
    }

    pub async fn test_webhook(&self) -> Result<String, String> {
        let config = self.get_config();
        if config.url.is_empty() {
            return Err("Webhook URL is not configured".to_string());
        }
        let now_bj = to_beijing_time(&Utc::now().to_rfc3339());
        let test_message = SmsMessage {
            id: 0,
            direction: "incoming".to_string(),
            phone_number: "+8613800138000".to_string(),
            content: "这是一条测试短信 (Webhook Test)".to_string(),
            timestamp: now_bj.clone(),
            status: "received".to_string(),
            pdu: None,
        };
        let payload = render_sms_template(&config.sms_template, &test_message);
        let mut request = self.client.post(&config.url);
        for (key, value) in &config.headers {
            request = request.header(key, value);
        }
        request = request.header("Content-Type", "application/json");
        if !config.secret.is_empty() {
            let signature = compute_signature(&config.secret, &payload);
            request = request.header("X-Webhook-Signature", signature);
        }
        let response = request
            .body(payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send test webhook: {}", e))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            Ok(format!("Webhook test successful (status: {})", status))
        } else {
            Err(format!("Webhook test failed (status: {}): {}", status, body))
        }
    }
}

fn render_sms_template(template: &str, message: &SmsMessage) -> String {
    let timestamp_cn = to_beijing_time(&message.timestamp);
    template
        .replace("{{id}}", &message.id.to_string())
        .replace("{{phone_number}}", &message.phone_number)
        .replace("{{content}}", &escape_json_string(&message.content))
        .replace("{{direction}}", &message.direction)
        .replace("{{timestamp}}", &timestamp_cn)
        .replace("{{status}}", &message.status)
        .replace("{{sender}}", &message.phone_number)
        .replace("{{message}}", &escape_json_string(&message.content))
        .replace("{{time}}", &timestamp_cn)
}

fn render_call_template(template: &str, call: &CallRecord) -> String {
    let answered_str = if call.answered { "是" } else { "否（未接）" };
    let direction_cn = match call.direction.as_str() {
        "incoming" => "来电",
        "outgoing" => "去电",
        "missed" => "未接来电",
        _ => "未知",
    };
    let start_time_cn = to_beijing_time(&call.start_time);
    let end_time_cn = call
        .end_time
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(to_beijing_time)
        .unwrap_or_else(|| "未结束".to_string());
    template
        .replace("{{id}}", &call.id.to_string())
        .replace("{{phone_number}}", &call.phone_number)
        .replace("{{direction}}", &call.direction)
        .replace("{{direction_cn}}", direction_cn)
        .replace("{{duration}}", &call.duration.to_string())
        .replace("{{start_time}}", &start_time_cn)
        .replace("{{end_time}}", &end_time_cn)
        .replace("{{answered}}", answered_str)
        .replace("{{answered_bool}}", &call.answered.to_string())
        .replace("{{caller}}", &call.phone_number)
        .replace("{{time}}", &start_time_cn)
}

fn escape_json_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn compute_signature(secret: &str, data: &str) -> String {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    format!("{}{}", secret, data).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
