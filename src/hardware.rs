use std::time::Duration;
use tokio::sync::mpsc;
use crate::models::EventPayload;

#[cfg(target_arch = "aarch64")]
pub async fn start_hardware_monitor(tx: mpsc::Sender<EventPayload>) {
    use rppal::gpio::{Gpio, Trigger};
    use chrono::Utc;
    
    // We assume the reed switch is connected to GPIO 17
    const REED_SWITCH_PIN: u8 = 17;
    
    // Roda em uma thread separada para não bloquear o executor do Tokio
    tokio::task::spawn_blocking(move || {
        let gpio = Gpio::new().expect("Failed to init GPIO");
        let mut pin = gpio.get(REED_SWITCH_PIN).expect("Failed to get pin").into_input_pullup();
        
        pin.set_interrupt(Trigger::Both, Some(Duration::from_millis(200))).expect("Failed to set interrupt");
        
        println!("Hardware monitor started on PIN {}", REED_SWITCH_PIN);
        
        loop {
            // poll_interrupt is a blocking call, timeout every 1 hour to check
            match pin.poll_interrupt(true, Some(Duration::from_secs(3600))) {
                Ok(Some(_event)) => {
                    let is_open = pin.is_high(); // Depende do wiring físico
                    let event_type = if is_open { "box_opened" } else { "box_closed" };
                    
                    let event = EventPayload {
                        event_type: event_type.to_string(),
                        timestamp: Utc::now().timestamp(),
                        metadata: serde_json::json!({
                            "pin": REED_SWITCH_PIN
                        }),
                    };
                    
                    if let Err(e) = tx.blocking_send(event) {
                        eprintln!("Failed to send event to queue: {}", e);
                        break; // Se o canal fechar, encerra a thread
                    }
                },
                Ok(None) => {
                    // Timeout (1 hora passou sem interrupções), continua o loop
                },
                Err(e) => {
                    eprintln!("GPIO Interrupt error: {}", e);
                }
            }
        }
    }).await.unwrap();
}

#[cfg(not(target_arch = "aarch64"))]
pub async fn start_hardware_monitor(tx: mpsc::Sender<EventPayload>) {
    use chrono::Utc;
    
    println!("MOCK Hardware monitor started (x86_64)");
    
    // Simulate events every 30 seconds for testing
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        
        let event = EventPayload {
            event_type: "box_opened".to_string(),
            timestamp: Utc::now().timestamp(),
            metadata: serde_json::json!({
                "mocked": true
            }),
        };
        
        println!("Simulating box_opened event...");
        if let Err(e) = tx.send(event).await {
            eprintln!("Failed to send mock event to queue: {}", e);
        }
        
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        let event_close = EventPayload {
            event_type: "box_closed".to_string(),
            timestamp: Utc::now().timestamp(),
            metadata: serde_json::json!({
                "mocked": true
            }),
        };
        
        println!("Simulating box_closed event...");
        let _ = tx.send(event_close).await;
    }
}
