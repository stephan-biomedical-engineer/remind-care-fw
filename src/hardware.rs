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
fn get_chip_path() -> &'static str {
    if std::path::Path::new("/dev/gpiochip4").exists() {
        "/dev/gpiochip4" // Raspberry Pi 4/5
    } else {
        "/dev/gpiochip0" // Older models
    }
}

#[cfg(target_arch = "aarch64")]
pub async fn start_hardware_monitor(tx: mpsc::Sender<EventPayload>, mut cmd_rx: mpsc::Receiver<HardwareCommand>) {
    use chrono::Utc;
    use std::sync::{Arc, Mutex};
    use gpiocdev::line::{Direction, Bias, EdgeDetection, Value, EdgeKind};
    use gpiocdev::tokio::AsyncRequest;
    use gpiocdev::Request;
    
    // Pinos físicos (BCM)
    const BUZZER_PIN: u32 = 22;
    
    let chip_path = get_chip_path();
    
    let mut buzzer_req = match Request::builder()
        .on_chip(chip_path)
        .with_consumer("telemed_buzzer")
        .with_line(BUZZER_PIN)
        .with_direction(Direction::Output)
        .request()
    {
        Ok(req) => req,
        Err(e) => {
            eprintln!("⚠️ Falha ao inicializar Buzzer no pino {}: {}", BUZZER_PIN, e);
            // Fallback para não quebrar (mock req) é difícil, então podemos usar Option
            // mas como Request não tem default, abortaremos o app se o buzzer falhar ou mockaremos depois.
            panic!("Critical hardware failure: Buzzer unavailable.");
        }
    };
    
    let _ = buzzer_req.set_value(BUZZER_PIN, Value::Inactive);
    
    let alarm_active = Arc::new(Mutex::new(false));
    let alarm_active_clone = Arc::clone(&alarm_active);
    
    // Tarefa 1: Controlador do Alarme (Buzzer/LED)
    tokio::spawn(async move {
        loop {
            let is_active = *alarm_active_clone.lock().unwrap();
            
            if is_active {
                // Toca o buzzer
                let _ = buzzer_req.set_value(BUZZER_PIN, Value::Active);
                
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    cmd = cmd_rx.recv() => {
                        if let Some(HardwareCommand::StopAlarm) = cmd {
                            *alarm_active_clone.lock().unwrap() = false;
                            let _ = buzzer_req.set_value(BUZZER_PIN, Value::Inactive);
                            continue;
                        }
                    }
                }
                
                // Silêncio
                let _ = buzzer_req.set_value(BUZZER_PIN, Value::Inactive);
                
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
                if let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        HardwareCommand::StartAlarm => {
                            *alarm_active_clone.lock().unwrap() = true;
                            println!("Hardware: Alarme LIGADO!");
                        }
                        HardwareCommand::StopAlarm => {
                            *alarm_active_clone.lock().unwrap() = false;
                            let _ = buzzer_req.set_value(BUZZER_PIN, Value::Inactive);
                        }
                    }
                } else {
                    break;
                }
            }
        }
    });

    // Mapeamento dos 7 Compartimentos (Gavetas: 1..7 -> BCM)
    let compartment_pins: [(u32, u32); 7] = [
        (1, 5),
        (2, 6),
        (3, 13),
        (4, 19),
        (5, 26),
        (6, 17),
        (7, 27),
    ];
    
    // Tarefa 2: Monitor dos 7 Reed Switches (libgpiod Async uAPI v2)
    for (compartment_id, pin_num) in compartment_pins {
        let tx_clone = tx.clone();
        let alarm_active_reed = Arc::clone(&alarm_active);
        
        let req = match Request::builder()
            .on_chip(chip_path)
            .with_consumer(&format!("telemed_comp_{}", compartment_id))
            .with_line(pin_num)
            .with_edge_detection(EdgeDetection::BothEdges)
            .with_bias(Bias::PullUp)
            .with_debounce_period(Duration::from_millis(200))
            .request() 
        {
            Ok(r) => AsyncRequest::new(r),
            Err(e) => {
                eprintln!("⚠️ Falha ao registrar pino {} (Compartimento {}): {}. Pulando.", pin_num, compartment_id, e);
                continue;
            }
        };

        tokio::spawn(async move {
            println!("Hardware monitor started for Compartment {} on PIN {}", compartment_id, pin_num);
            
            loop {
                match req.read_edge_event().await {
                    Ok(event) => {
                        // Com PullUp interno, o pino fica HIGH quando desconectado (caixa aberta)
                        // Borda de Subida (Rising) = Abertura
                        // Borda de Descida (Falling) = Fechamento
                        let is_open = event.kind == EdgeKind::Rising;
                        let event_type = if is_open { "compartment_opened" } else { "compartment_closed" };
                        
                        if is_open {
                            *alarm_active_reed.lock().unwrap() = false;
                            println!("Hardware: Compartimento {} aberto! Alarme desarmado.", compartment_id);
                        }
                        
                        let ev_payload = EventPayload {
                            event_type: event_type.to_string(),
                            timestamp: Utc::now().timestamp(),
                            metadata: serde_json::json!({
                                "compartment": compartment_id,
                                "pin": pin_num
                            }),
                        };
                        
                        if let Err(e) = tx_clone.send(ev_payload).await {
                            eprintln!("Failed to send event to queue: {}", e);
                            break;
                        }
                    },
                    Err(e) => {
                        eprintln!("GPIO Async Interrupt error on pin {}: {}", pin_num, e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
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
            event_type: "compartment_opened".to_string(),
            timestamp: Utc::now().timestamp(),
            metadata: serde_json::json!({"compartment": 3, "mocked": true}),
        };
        
        println!("Simulating compartment_opened event (Compartment 3)...");
        let _ = tx_clone.send(event).await;
        
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        let event_close = EventPayload {
            event_type: "compartment_closed".to_string(),
            timestamp: Utc::now().timestamp(),
            metadata: serde_json::json!({"compartment": 3, "mocked": true}),
        };
        
        println!("Simulating compartment_closed event (Compartment 3)...");
        let _ = tx_clone.send(event_close).await;
    }
}
