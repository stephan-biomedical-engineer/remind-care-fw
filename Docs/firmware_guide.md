# Guia de Desenvolvimento do Firmware (RemindCare IoT) 📦🦀

Este documento define as regras de negócio, fluxos lógicos e integração de hardware que o firmware (escrito em Rust para a Raspberry Pi) deve seguir para garantir o funcionamento do ecossistema RemindCare.

> [!IMPORTANT]
> A placa não gerencia "Usuários". Ela não sabe a quem pertence. O mundo dela se resume a baixar o **Schedule (Agenda)** e enviar **Eventos (Abertura de Caixa)** para o servidor. Todo o pareamento é responsabilidade exclusiva do Servidor e do App Mobile.

---

## 1. Identidade e Provisionamento (Boot)

A placa nunca deve ter chaves hardcoded no código fonte. Quando a placa ligar, o firmware deve obrigatoriamente ler o arquivo `.env.production` presente na raiz do executável.

**Variáveis esperadas:**
```env
API_URL=https://citable-sharpness-scalping.ngrok-free.dev/api/v1/devices
API_KEY=ZA66TsCZH0OPtYRkkxAb6XbMKzOqace85PYcULXEQKwXa90Q
DEVICE_ID=RC-CB0928
```
Todas as requisições HTTP feitas ao servidor devem incluir o cabeçalho:
`Authorization: Bearer <API_KEY>`

---

## 2. O Ciclo de Vida da Rede (Network Worker)

O firmware deve ter uma thread dedicada para comunicação assíncrona (ex: `tokio::spawn`).

### A) O Heartbeat (Sinal de Vida)
A cada 60 segundos, a placa deve enviar um `POST /heartbeat` informando o `uptime_seconds`.
* **Se o servidor retornar `404 Not Found`**: Significa que a placa ainda não foi pareada pelo usuário no aplicativo. **Ação:** O firmware deve apenas dormir por mais 60 segundos. **NÃO tente baixar o schedule para não gerar spam no servidor.**
* **Se o servidor retornar `200 OK`**: A placa está pareada e operante.

### B) Sincronização do Schedule (Agenda)
A resposta de um Heartbeat bem-sucedido (200 OK) será: `{"status": "ok", "schedule_updated": true/false}`.
* **SE `schedule_updated` for `true`** (ou se for o primeiro boot pós-pareamento): A placa deve fazer um `GET /schedule`.
* O JSON recebido com os horários e compartimentos deve ser salvo imediatamente no banco de dados local (SQLite) da placa.

---

## 3. Resiliência Offline e Eventos (Sync Worker)

A internet da casa do paciente não é confiável. O dispositivo deve ser **Offline-First**.

* **Geração de Evento:** Quando uma gaveta for aberta, o firmware não deve tentar enviar o POST direto para a nuvem. Ele deve primeiro fazer um `INSERT` na tabela local do SQLite `pending_events`.
* **Sync Loop:** Uma thread rodando a cada 10 segundos deve olhar para a tabela `pending_events`. Se houver dados e houver internet, envia um `POST /events`.
* **Confirmação:** Apenas quando o servidor responder `201 Created`, a placa deve fazer o `DELETE` desse evento no SQLite local. Se der erro (ex: sem internet), o evento fica guardado em disco e será tentado novamente mais tarde. Nada se perde.

---

## 4. Integração Física (Hardware & GPIO)

A caixa atua no mundo real baseado no Schedule armazenado em seu SQLite.

### A) O Relógio Interno e Sinalização
Uma thread (Loop) deve ler o relógio do sistema de segundo a segundo (garantir que o timezone da Raspberry Pi e NTP estejam sincronizados).
* Quando a `hora_atual` for igual à `hora_do_remedio` (da tabela schedule), a placa deve **acionar um Buzzer e piscar um LED**.

### B) Atuação Mecânica (Travas)
Para garantir a segurança, os compartimentos da caixa devem ficar travados (ex: via travas solenoides ou servomotores).
* No horário correto, se o schedule disser `compartment: 1`, apenas o pino GPIO correspondente à trava 1 deve ser acionado para liberar a gaveta.

### C) Sensores Magnéticos (Reed Switches)
Cada porta/gaveta da caixa deve possuir um sensor magnético (Reed Switch) conectado a um GPIO configurado com resistor Pull-Up (ou Pull-Down interno).
* O firmware deve detectar interrupções (Borda de subida/descida) de forma não bloqueante.
* Ao detectar a abertura física da porta `X`, o firmware cria o payload JSON com `event_type: "box_opened"` e o respectivo `compartment`, e injeta isso na fila (SQLite) do Sync Worker.
