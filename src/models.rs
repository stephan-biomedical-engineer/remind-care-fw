#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Clone)]
pub struct HeartbeatRequest {
    pub uptime_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_strength_dbm: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsynced_events: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct HeartbeatResponse {
    pub status: String,
    pub schedule_updated: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MedicationSchedule {
    pub medication_id: u32,
    pub name: String,
    pub dosage: String,
    pub time: String,
    pub compartment: u32,
    pub week_days: Vec<u8>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ScheduleResponse {
    pub device_id: String,
    pub schedule: Vec<MedicationSchedule>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventPayload {
    pub event_type: String,
    pub timestamp: i64,
    pub metadata: serde_json::Value,
}

#[derive(Serialize, Debug, Clone)]
pub struct LogPayload {
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub message: String,
    pub timestamp: i64,
}
