# Contrato de Integração de Firmware (Hardware ↔ Servidor) 📡

Este documento serve como contrato de comunicação para o **Engenheiro de Firmware/Embarcados** responsável pelo código da Raspberry Pi (ou microcontrolador similar). Ele define como o hardware deve interagir com a API em nuvem (VPS).

---

## ⚙️ 1. Princípios Arquiteturais

*   **Padrão de Comunicação:** A caixa IoT atua **exclusivamente como Cliente HTTP (Pull-based)**. A API em nuvem nunca tentará abrir uma conexão direta (push) para o hardware, contornando assim problemas com firewalls residenciais e roteadores NAT.
*   **URL Base:** `https://<dominio-do-servidor>/api/v1/devices`
*   **Formato de Dados:** Todas as requisições e respostas usam JSON (`Content-Type: application/json`).

---

## 🔒 2. Autenticação (API Key Estática)

A caixa não possui teclado ou usuário humano para realizar login. Portanto, o acesso é garantido via uma **API Key Estática**.

*   **Gravação na Fábrica:** O dispositivo já deve sair de fábrica com a `device_id` (ex: MAC Address ou Número de Série) e a respectiva `API_KEY` gravadas no firmware ou em uma partição segura.
*   **Header Obrigatório:** Todas as requisições listadas abaixo devem enviar a API Key no formato Bearer token:
    ```http
    Authorization: Bearer <API_KEY_DO_DISPOSITIVO>
    ```
> [!WARNING]
> Se o servidor retornar o código HTTP `401 Unauthorized`, significa que a API Key é inválida, o dispositivo foi desativado (por suspeita de roubo) ou excluído do banco. Nesse caso, a caixa deve parar de enviar requisições por um longo período para economizar bateria/banda.

---

## 🛣️ 3. Endpoints do Dispositivo

### 1. Heartbeat (Sinal de Vida e Estado do Hardware)

A caixa deve enviar este pacote periodicamente para avisar à nuvem que está operante e reportar a saúde do sistema.

*   **Método:** `POST`
*   **Rota:** `/heartbeat`
*   **Quando enviar:** A cada X minutos configurados (ex: a cada 30 minutos).
*   **Corpo da Requisição:**
    ```json
    {
      "uptime_seconds": 86400,          // Tempo ligado desde o último boot (inteiro)
      "network_strength_dbm": -65,      // Sinal do Wi-Fi (opcional)
      "firmware_version": "1.2.4",      // Versão atual do código (opcional)
      "unsynced_events": 0              // Quantos eventos offline estão na fila (opcional)
    }
    ```
*   **Respostas Esperadas:**
    *   `200 OK`:
        ```json
        {
          "status": "ok",
          "schedule_updated": true 
        }
        ```
> [!TIP]
> **Otimização de Bateria:** Fique atento à variável `schedule_updated`. Se o servidor retornar `true`, significa que o médico alterou a receita do paciente enquanto a caixa estava ociosa. O firmware deve **imediatamente** engatilhar uma requisição `GET /schedule` para baixar a nova agenda! Se retornar `false`, a caixa pode voltar a dormir em paz.

---

### 2. Schedule (Agenda Médica)

A caixa baixa a agenda completa de medicamentos do paciente vinculado a ela. 

*   **Método:** `GET`
*   **Rota:** `/schedule`
*   **Quando enviar:** No boot do SO, após recuperar a conexão com a internet, ou se o `Heartbeat` avisar que a agenda mudou.
*   **Respostas Esperadas:**
    *   `200 OK`: Retorna os horários e compartimentos que devem abrir.
        ```json
        {
          "device_id": "CX-998877",
          "schedule": [
            {
              "medication_id": "770e8400-e29b-41d4-a716-446655440000",
              "name": "Paracetamol",
              "dosage": "500mg",
              "time": "14:00:00",
              "compartment": 1,
              "week_days": [1, 3, 5] // 0=Domingo, 1=Segunda, etc. (Depende da implementação do banco)
            }
          ]
        }
        ```
    *   `404 Not Found`: "Device not bound to any user". A caixa ainda não foi pareada com o celular de nenhum paciente. Ela deve piscar um LED aguardando o usuário pareá-la.

---

### 3. Events (Telemetria de Sensores)

Canal principal onde a caixa reporta interações físicas.

*   **Método:** `POST`
*   **Rota:** `/events`
*   **Quando enviar:** Imediatamente após uma ação física.
*   **Comportamento Offline:** Se a caixa estiver sem internet, **salve esse JSON na memória/SD**. Quando a rede voltar, dispare os pacotes retidos (respeitando o `timestamp` original em que o evento ocorreu no hardware).
*   **Corpo da Requisição:**
    ```json
    {
      "event_type": "box_opened",    // Tipos comuns: "box_opened", "box_closed", "tamper_detected"
      "timestamp": 1719803763,       // Unix Timestamp (segundos) gerado no momento do evento!
      "metadata": {                  // Objeto livre para enviar dados extras úteis
        "duration_open_ms": 5000,
        "compartment_opened": 1
      }
    }
    ```
*   **Respostas Esperadas:**
    *   `201 Created`: O servidor salvou com sucesso. O firmware pode apagar o evento da fila local.

---

### 4. Logs (Auditoria de Erros)

Para facilitar o debug remoto sem precisar acessar via SSH.

*   **Método:** `POST`
*   **Rota:** `/logs`
*   **Quando enviar:** Quando uma exceção não tratada ocorre (falha em sensor, erro I2C).
*   **Corpo da Requisição:**
    ```json
    {
      "level": "ERROR",                 // "ERROR", "WARN" ou "INFO"
      "component": "sensor_hall_i2c",   // Onde ocorreu o erro (opcional)
      "message": "Failed to read data from bus 1",
      "timestamp": 1719803800           // Unix Timestamp da ocorrência
    }
    ```
*   **Respostas Esperadas:**
    *   `201 Created`: O log foi salvo no servidor.
