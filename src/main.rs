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

struct ActiveAlarmState {
    medication_id: String,
    compartment: u32,
    triggered_at: chrono::DateTime<chrono::Utc>,
    buzzer_active: bool,
}

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

    let active_alarm = std::sync::Arc::new(tokio::sync::Mutex::new(None::<ActiveAlarmState>));
    let taken_medications = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::<String>::new()));

    // 1. Task: Hardware Monitor
    let tx_hardware = tx.clone();
    tokio::spawn(async move {
        start_hardware_monitor(tx_hardware, cmd_rx).await;
    });

    // 2. Task: Local Event Persister (Queue Manager)
    let active_alarm_clone1 = active_alarm.clone();
    let taken_meds_clone = taken_medications.clone();
    
    tokio::spawn(async move {
        while let Some(mut event) = rx.recv().await {
            println!("Received hardware event: {}", event.event_type);
            
            if event.event_type == "compartment_opened" {
                if let Some(comp_val) = event.metadata.get("compartment") {
                    if let Some(compartment_opened) = comp_val.as_u64() {
                        let mut alarm_guard = active_alarm_clone1.lock().await;
                        
                        // Check if it's the active alarm (to clear it and silence)
                        if let Some(active) = alarm_guard.as_mut() {
                            if active.compartment == compartment_opened as u32 {
                                println!("✅ Medicine {} taken! Active alarm cleared.", active.medication_id);
                                *alarm_guard = None; // Clears the alarm
                            } else {
                                println!("⚠️ Wrong compartment {} opened while alarm was for compartment {}", compartment_opened, active.compartment);
                            }
                        }
                        
                        // Now calculate clinical status
                        if let Ok(Some(schedule_resp)) = store_clone1.load_schedule().await {
                            use chrono::Datelike;
                            let now = chrono::Local::now();
                            let current_weekday = now.weekday().number_from_monday() as u8;
                            
                            // Find medicine for this compartment today
                            let mut closest_med = None;
                            let mut smallest_diff: i64 = i64::MAX;
                            
                            for med in schedule_resp.schedule {
                                if med.compartment == compartment_opened as u32 && med.week_days.contains(&current_weekday) {
                                    if let Ok(sched_time) = chrono::NaiveTime::parse_from_str(&med.time, "%H:%M:%S") {
                                        let diff = now.time().signed_duration_since(sched_time).num_minutes();
                                        if diff.abs() < smallest_diff.abs() {
                                            smallest_diff = diff;
                                            closest_med = Some(med.clone());
                                        }
                                    } else if let Ok(sched_time) = chrono::NaiveTime::parse_from_str(&med.time, "%H:%M") {
                                        let diff = now.time().signed_duration_since(sched_time).num_minutes();
                                        if diff.abs() < smallest_diff.abs() {
                                            smallest_diff = diff;
                                            closest_med = Some(med.clone());
                                        }
                                    }
                                }
                            }
                            
                            if let Some(med) = closest_med {
                                let today_str = now.format("%Y-%m-%d").to_string();
                                let debounce_key = format!("{}-{}", med.medication_id, today_str);
                                
                                let mut taken_guard = taken_meds_clone.lock().await;
                                if taken_guard.contains(&debounce_key) {
                                    println!("Debounce: Status clínico do medicamento {} já foi registrado hoje. Ignorando evento clínico duplicado.", med.medication_id);
                                } else {
                                    taken_guard.insert(debounce_key);

                                    let situation = if smallest_diff < -15 {
                                        "early"
                                    } else if smallest_diff <= 15 {
                                        "onTime"
                                    } else if smallest_diff <= 45 {
                                        "warning"
                                    } else if smallest_diff <= 60 {
                                        "late"
                                    } else {
                                        "missed"
                                    };
                                    
                                    println!("📊 Clinical Event for Compartment {}: {} (Diff: {} min)", compartment_opened, situation, smallest_diff);
                                    
                                    // Override the event to send medication_status
                                    event.event_type = "medication_status".to_string();
                                    event.metadata = serde_json::json!({
                                        "medication_id": med.medication_id,
                                        "situation": situation,
                                        "compartment": compartment_opened
                                    });
                                }
                            }
                        }
                    }
                }
            }

            if let Err(e) = store_clone1.push_event(&event).await {
                eprintln!("Failed to save event to local DB: {}", e);
            }
        }
    });

    let store_clone3 = store.clone();
    let cmd_tx_clone = cmd_tx.clone();
    let active_alarm_clone2 = active_alarm.clone();

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
                            
                            let mut alarm_guard = active_alarm_clone2.lock().await;
                            *alarm_guard = Some(ActiveAlarmState {
                                medication_id: med.medication_id.clone(),
                                compartment: med.compartment,
                                triggered_at: chrono::Utc::now(),
                                buzzer_active: true,
                            });
                            
                            let _ = cmd_tx_clone.send(hardware::HardwareCommand::StartAlarm).await;
                        }
                    }
                }
            }
        }
    });

    // 6. Task: Alarm Timeout Monitor
    let active_alarm_timeout = active_alarm.clone();
    let cmd_tx_timeout = cmd_tx.clone();
    let tx_timeout = tx.clone();
    
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            
            let mut alarm_guard = active_alarm_timeout.lock().await;
            if let Some(active) = alarm_guard.as_mut() {
                let now = chrono::Utc::now();
                let elapsed_minutes = now.signed_duration_since(active.triggered_at).num_minutes();
                let elapsed_seconds = now.signed_duration_since(active.triggered_at).num_seconds();
                
                // Estágio 1: Silenciador de Curta Duração (60 segundos)
                if active.buzzer_active && elapsed_seconds >= 60 {
                    println!("🔕 Alarme silenciado (curta duração), mas janela de medicação continua aberta.");
                    let _ = cmd_tx_timeout.send(hardware::HardwareCommand::StopAlarm).await;
                    active.buzzer_active = false;
                }

                // Estágio 2: Fim da Tolerância Clínica (2 minutos para teste rápido)
                if elapsed_minutes >= 2 {
                    println!("🚨 JANELA FECHADA! Paciente perdeu a medicação {}.", active.medication_id);
                    
                    let event = EventPayload {
                        event_type: "medication_status".to_string(),
                        timestamp: now.timestamp(),
                        metadata: serde_json::json!({
                            "medication_id": active.medication_id,
                            "situation": "missed",
                            "compartment": active.compartment
                        }),
                    };
                    let _ = tx_timeout.send(event).await;

                    // Desliga fisicamente por segurança caso ainda estivesse tocando
                    if active.buzzer_active {
                        let _ = cmd_tx_timeout.send(hardware::HardwareCommand::StopAlarm).await;
                    }

                    *alarm_guard = None;
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
