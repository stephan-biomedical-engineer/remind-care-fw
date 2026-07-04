mod api;
mod hardware;
mod models;
mod store;

use std::time::Duration;
use tokio::sync::mpsc;

use api::ApiClient;
use hardware::start_hardware_monitor;
use models::{EventPayload, HeartbeatRequest};
use store::Store;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables (.env)
    dotenvy::dotenv().ok();

    println!("Initializing Telemed OS...");

    // Initialize local database
    let store = Store::new("telemed_local.db").await?;
    let store_clone1 = store.clone();
    let store_clone2 = store.clone();

    // Initialize API Client
    let api_client = ApiClient::new();
    let api_client_clone = api_client.clone();

    // Channels for events and hardware commands
    let (tx, mut rx) = mpsc::channel::<EventPayload>(100);
    let (cmd_tx, cmd_rx) = mpsc::channel::<hardware::HardwareCommand>(10);

    // 1. Task: Hardware Monitor
    tokio::spawn(async move {
        start_hardware_monitor(tx, cmd_rx).await;
    });

    // 2. Task: Local Event Persister (Queue Manager)
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            println!("Received hardware event: {}", event.event_type);
            if let Err(e) = store_clone1.push_event(&event).await {
                eprintln!("Failed to save event to local DB: {}", e);
            }
        }
    });

    let store_clone3 = store.clone();
    let cmd_tx_clone = cmd_tx.clone();

    // 5. Task: Scheduler (Relógio Interno)
    tokio::spawn(async move {
        use chrono::Datelike;
        let mut last_alarm_time = String::new();
        
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            let now = chrono::Local::now();
            let current_time_str = now.format("%H:%M").to_string(); // "HH:MM"
            let current_weekday = now.weekday().number_from_monday() as u8; // 1 = Mon, 7 = Sun
            
            if let Ok(Some(schedule_resp)) = store_clone3.load_schedule().await {
                for med in schedule_resp.schedule {
                    if med.time.starts_with(&current_time_str) && med.week_days.contains(&current_weekday) {
                        if last_alarm_time != current_time_str {
                            println!("⏰ ALARME! Hora de tomar: {} ({})", med.name, med.dosage);
                            last_alarm_time = current_time_str.clone();
                            let _ = cmd_tx_clone.send(hardware::HardwareCommand::StartAlarm).await;
                        }
                    }
                }
            }
        }
    });

    // 3. Task: API Event Sync Worker
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            match store_clone2.get_unsynced_events().await {
                Ok(events) => {
                    for (id, event) in events {
                        println!("Attempting to sync event id {} to API...", id);
                        match api_client.post_event(&event).await {
                            Ok(_) => {
                                println!("Successfully synced event {}.", id);
                                let _ = store_clone2.delete_event(id).await;
                            }
                            Err(e) => {
                                eprintln!("Failed to sync event {}: {}", id, e);
                                // Stop trying the rest if network is down
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read unsynced events: {}", e);
                }
            }
        }
    });

    // 4. Task: Heartbeat & Schedule Worker
    // This runs in the main thread (or we could spawn it and await a ctrl-c)
    loop {
        println!("Sending Heartbeat...");
        let uptime = 0; // TODO: properly calculate uptime
        let unsynced = store.get_unsynced_count().await.unwrap_or(0);
        
        let req = HeartbeatRequest {
            uptime_seconds: uptime,
            network_strength_dbm: None,
            firmware_version: Some("0.1.0".to_string()),
            unsynced_events: Some(unsynced),
        };

        match api_client_clone.send_heartbeat(&req).await {
            Ok(resp) => {
                println!("Heartbeat OK. schedule_updated = {}", resp.schedule_updated);
                
                if resp.schedule_updated {
                    println!("Fetching new schedule...");
                    match api_client_clone.fetch_schedule().await {
                        Ok(schedule) => {
                            if let Err(e) = store.save_schedule(&schedule).await {
                                eprintln!("Failed to save schedule to local DB: {}", e);
                            } else {
                                println!("Schedule saved. Got {} medications.", schedule.schedule.len());
                            }
                        }
                        Err(e) => eprintln!("Failed to fetch schedule: {}", e),
                    }
                } else if store.load_schedule().await.unwrap_or(None).is_none() {
                    println!("No local schedule. Attempting first fetch...");
                    if let Ok(schedule) = api_client_clone.fetch_schedule().await {
                        let _ = store.save_schedule(&schedule).await;
                        println!("Schedule saved. Got {} medications.", schedule.schedule.len());
                    }
                }
            }
            Err(e) => {
                eprintln!("Heartbeat failed: {}", e);
            }
        }

        // Wait 30 minutes (using 30 seconds for test purposes)
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
