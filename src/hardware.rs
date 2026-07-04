use std::time::Duration;
use tokio::sync::mpsc;
use crate::models::EventPayload;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareCommand {
    StartAlarm,
    StopAlarm,
}

#[cfg(target_arch = "aarch64")]
pub async fn start_hardware_monitor(tx: mpsc::Sender<EventPayload>, mut cmd_rx: mpsc::Receiver<HardwareCommand>) {
    use rppal::gpio::{Gpio, Trigger};
    use chrono::Utc;
    use std::sync::{Arc, Mutex};
    
    // Pinos físicos
    const REED_SWITCH_PIN: u8 = 17;
    const BUZZER_PIN: u8 = 22;
    
    let gpio = Gpio::new().expect("Failed to init GPIO");
    let mut reed_pin = gpio.get(REED_SWITCH_PIN).expect("Failed to get reed pin").into_input_pullup();
    let mut buzzer_pin = gpio.get(BUZZER_PIN).expect("Failed to get buzzer pin").into_output();
    buzzer_pin.set_low();
    
    let alarm_active = Arc::new(Mutex::new(false));
    let alarm_active_clone = Arc::clone(&alarm_active);
    
    // Tarefa 1: Controlador do Alarme (Buzzer/LED)
    tokio::spawn(async move {
        loop {
            let is_active = *alarm_active_clone.lock().unwrap();
            
            if is_active {
                // Toca o buzzer
                buzzer_pin.set_high();
                
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    cmd = cmd_rx.recv() => {
                        if let Some(HardwareCommand::StopAlarm) = cmd {
                            *alarm_active_clone.lock().unwrap() = false;
                            buzzer_pin.set_low();
                            continue;
                        }
                    }
                }
                
                // Silêncio
                buzzer_pin.set_low();
                
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    cmd = cmd_rx.recv() => {
                        if let Some(HardwareCommand::StopAlarm) = cmd {
                            *alarm_active_clone.lock().unwrap() = false;
                            continue;
                        }
                    }
                }
            } else {
                // Fica aguardando novos comandos silenciosamente (não gasta CPU)
                if let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        HardwareCommand::StartAlarm => {
                            *alarm_active_clone.lock().unwrap() = true;
                            println!("Hardware: Alarme LIGADO!");
                        }
                        HardwareCommand::StopAlarm => {
                            *alarm_active_clone.lock().unwrap() = false;
                            buzzer_pin.set_low();
                        }
                    }
                } else {
                    break; // O canal fechou
                }
            }
        }
    });

    let tx_clone = tx.clone();
    let alarm_active_reed = Arc::clone(&alarm_active);
    
    // Tarefa 2: Monitor do Reed Switch (Thread Dedicada e Bloqueante)
    tokio::task::spawn_blocking(move || {
        reed_pin.set_interrupt(Trigger::Both, Some(Duration::from_millis(200))).expect("Failed to set interrupt");
        println!("Hardware monitor started on PIN {} (Reed) and PIN {} (Buzzer)", REED_SWITCH_PIN, BUZZER_PIN);
        
        loop {
            match reed_pin.poll_interrupt(true, Some(Duration::from_secs(3600))) {
                Ok(Some(_event)) => {
                    let is_open = reed_pin.is_high(); // Depende do wiring físico
                    let event_type = if is_open { "box_opened" } else { "box_closed" };
                    
                    if is_open {
                        // Se a caixa abrir, silencia o alarme instantaneamente!
                        *alarm_active_reed.lock().unwrap() = false;
                        println!("Hardware: Caixa aberta! Alarme desarmado.");
                    }
                    
                    let event = EventPayload {
                        event_type: event_type.to_string(),
                        timestamp: Utc::now().timestamp(),
                        metadata: serde_json::json!({
                            "pin": REED_SWITCH_PIN
                        }),
                    };
                    
                    if let Err(e) = tx_clone.blocking_send(event) {
                        eprintln!("Failed to send event to queue: {}", e);
                        break;
                    }
                },
                Ok(None) => {},
                Err(e) => eprintln!("GPIO Interrupt error: {}", e),
            }
        }
    });
}

#[cfg(not(target_arch = "aarch64"))]
pub async fn start_hardware_monitor(tx: mpsc::Sender<EventPayload>, mut cmd_rx: mpsc::Receiver<HardwareCommand>) {
    use chrono::Utc;
    use std::sync::{Arc, Mutex};
    
    println!("MOCK Hardware monitor started (x86_64)");
    
    let alarm_active = Arc::new(Mutex::new(false));
    let alarm_active_clone = Arc::clone(&alarm_active);
    
    tokio::spawn(async move {
        loop {
            let is_active = *alarm_active_clone.lock().unwrap();
            
            if is_active {
                println!("MOCK: BEEP!");
                
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(1000)) => {}
                    cmd = cmd_rx.recv() => {
                        if let Some(HardwareCommand::StopAlarm) = cmd {
                            *alarm_active_clone.lock().unwrap() = false;
                            println!("MOCK: Alarm STOPPED");
                            continue;
                        }
                    }
                }
            } else {
                if let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        HardwareCommand::StartAlarm => {
                            *alarm_active_clone.lock().unwrap() = true;
                            println!("MOCK: Alarm STARTED!");
                        }
                        HardwareCommand::StopAlarm => {
                            *alarm_active_clone.lock().unwrap() = false;
                        }
                    }
                } else {
                    break;
                }
            }
        }
    });
    
    let tx_clone = tx.clone();
    let alarm_active_reed = Arc::clone(&alarm_active);
    
    // Simulate events every 60 seconds
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        
        let is_alarming = *alarm_active_reed.lock().unwrap();
        if is_alarming {
            *alarm_active_reed.lock().unwrap() = false;
            println!("MOCK: User opened the box because of the alarm! Stopping alarm.");
        }
        
        let event = EventPayload {
            event_type: "box_opened".to_string(),
            timestamp: Utc::now().timestamp(),
            metadata: serde_json::json!({"mocked": true}),
        };
        
        println!("Simulating box_opened event...");
        let _ = tx_clone.send(event).await;
        
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        let event_close = EventPayload {
            event_type: "box_closed".to_string(),
            timestamp: Utc::now().timestamp(),
            metadata: serde_json::json!({"mocked": true}),
        };
        
        println!("Simulating box_closed event...");
        let _ = tx_clone.send(event_close).await;
    }
}
