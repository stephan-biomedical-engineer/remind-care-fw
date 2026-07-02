#![allow(dead_code)]
use reqwest::{Client, StatusCode};
use std::env;
use crate::models::{HeartbeatRequest, HeartbeatResponse, ScheduleResponse, EventPayload, LogPayload};

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl ApiClient {
    pub fn new() -> Self {
        let base_url = env::var("API_URL").expect("API_URL must be set");
        let api_key = env::var("API_KEY").expect("API_KEY must be set");
        
        Self {
            client: Client::new(),
            base_url,
            api_key,
        }
    }

    pub async fn send_heartbeat(&self, req: &HeartbeatRequest) -> Result<HeartbeatResponse, String> {
        let url = format!("{}/heartbeat", self.base_url);
        let resp = self.client.post(&url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await
            .map_err(|e| e.to_string())?;
            
        if resp.status().is_success() {
            let heartbeat_resp = resp.json::<HeartbeatResponse>().await.map_err(|e| e.to_string())?;
            Ok(heartbeat_resp)
        } else {
            Err(format!("Heartbeat failed with status: {}", resp.status()))
        }
    }

    pub async fn fetch_schedule(&self) -> Result<ScheduleResponse, String> {
        let url = format!("{}/schedule", self.base_url);
        let resp = self.client.get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            let schedule_resp = resp.json::<ScheduleResponse>().await.map_err(|e| e.to_string())?;
            Ok(schedule_resp)
        } else if resp.status() == StatusCode::NOT_FOUND {
            Err("Device not bound to any user (404)".to_string())
        } else {
            Err(format!("Schedule fetch failed with status: {}", resp.status()))
        }
    }

    pub async fn post_event(&self, event: &EventPayload) -> Result<(), String> {
        let url = format!("{}/events", self.base_url);
        let resp = self.client.post(&url)
            .bearer_auth(&self.api_key)
            .json(event)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Event post failed with status: {}", resp.status()))
        }
    }

    pub async fn post_log(&self, log: &LogPayload) -> Result<(), String> {
        let url = format!("{}/logs", self.base_url);
        let resp = self.client.post(&url)
            .bearer_auth(&self.api_key)
            .json(log)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Log post failed with status: {}", resp.status()))
        }
    }
}
